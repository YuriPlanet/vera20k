//! Telemetry snapshot capture for the browser dashboard.
//!
//! Assembles the whole wire payload from live app state: the simulation-side
//! capture (`Simulation::telemetry_snapshot`) plus the app-only facts —
//! frame timing, cursor cell, walkability, selected movers' paths and miner
//! state. Per-entity event history is available only with `debug-tools`.
//!
//! The app owns diagnostic samples and log delivery; gameplay state is read-only.

use crate::app::{AppState, BUILD_STAMP};
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::SpeedType;
use crate::sim::movement::motion_query;
use crate::sim::telemetry::{
    DebugPanel, EntityDesc, LandRow, MinerDebug, PathStep, ProcessStats, SelectedDebug,
    SessionContext, TELEMETRY_MAX_NAME_BYTES, TelemetrySnapshot,
};
use crate::sim::world::Simulation;

// Per-snapshot work and payload limits for the app-only detail pane.
const MAX_SELECTED: usize = 32;
const MAX_AT_CURSOR: usize = 32;
const MAX_PATH_PREVIEW: usize = 8;
const MAX_DETAIL_BYTES: usize = 256;
const PUBLISH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// World replacement starts a new observation window without touching the world.
pub(crate) fn reset_observation(state: &mut AppState) {
    #[cfg(feature = "debug-tools")]
    crate::app::debug_tools::reset_session(state);
    reset_live_window(state);
    state.match_state.telemetry_history.clear();
    state.match_state.report_history.clear();
    state.diag.render_report_cache = None;
    state.diag.telemetry_history_generation = 0;
    // A replaced world has a new elevation grid: drop the cache and force the
    // next publish to resend it, even if the same client stays attached.
    state.diag.telemetry_terrain = None;
    state.diag.telemetry_terrain_dirty = true;
}

/// The attach/detach boundary: the live window restarts, but the whole-match
/// history must survive it — that is exactly the time a late client needs.
fn reset_live_window(state: &mut AppState) {
    state.match_state.telemetry_timing.clear();
    state.match_state.telemetry_tick_rate.clear();
    state.diag.render_timing = Default::default();
    state.diag.telemetry_last_publish = None;
}

pub(crate) fn synchronize_observer(state: &mut AppState) {
    let active = state.diag.telemetry_active();
    let simulation = state
        .match_state
        .sim_runtime
        .as_mut()
        .map(|rt| &mut rt.simulation);
    if state
        .diag
        .telemetry_observer
        .synchronize(simulation, active)
    {
        reset_live_window(state);
    }
}

fn bounded_text(value: &str, maximum: usize) -> String {
    let mut end = value.len().min(maximum);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// Capture one full dashboard snapshot, or `None` when no match is running.
pub fn capture_snapshot(state: &AppState) -> Option<TelemetrySnapshot> {
    let runtime = state.match_state.sim_runtime.as_ref()?;
    let mut snapshot = runtime.simulation.telemetry_snapshot();
    #[cfg(feature = "debug-tools")]
    {
        snapshot.debug_tools.session = snapshot
            .debug_tools
            .enabled
            .then_some(state.diag.debug_session_epoch)
            .filter(|epoch| *epoch != 0);
    }
    // House colours: the same `[Colors]` scheme map the renderer tints units
    // with, so the dashboard shows each player's real team colour instead of a
    // dashboard-invented palette.
    let schemes = &runtime.resources.rules.color_schemes;
    let house_colors = &state.match_state.match_presentation.house_color_map;
    for house in &mut snapshot.houses {
        house.color = house_colors
            .get(&house.name)
            .map(|index| crate::rules::house_colors::house_text_rgb(schemes, *index));
        // The side's own `[Sides]` key — the index is just its position there.
        house.side_name = u8::try_from(house.side_index)
            .ok()
            .and_then(|index| {
                runtime
                    .resources
                    .rules
                    .side_name(crate::rules::ruleset::SideIdx(index))
            })
            .map(|name| bounded_text(name, TELEMETRY_MAX_NAME_BYTES));
    }
    let speed = runtime
        .simulation
        .session()
        .game_options()
        .game_speed
        .clamp(0, 6) as u8;
    let mut timing =
        state
            .match_state
            .telemetry_timing
            .summarize(state.platform.frame_pacer.budget_ms(
                speed,
                crate::app::match_runtime::network_match::frame_budget(state),
            ));
    // Each rate keeps its own owner: FPS is the render loop's, tick_hz is the
    // committed-tick meter's.
    timing.fps = state.diag.frame_timer.fps();
    timing.tick_hz = state
        .match_state
        .telemetry_tick_rate
        .rate(crate::util::time::Instant::now());
    snapshot.timing = Some(timing);
    let sim = &runtime.simulation;
    snapshot.session = Some(SessionContext {
        map_name: bounded_text(sim.session().map_name(), TELEMETRY_MAX_NAME_BYTES),
        // The app's GameMode holder, not the world's copy read a second way.
        game_mode: state.match_state.session_mode().as_str().to_owned(),
        game_speed: speed,
        local_owner: state
            .match_state
            .local_player_owner()
            .map(|owner| bounded_text(owner, TELEMETRY_MAX_NAME_BYTES)),
        local_outcome: sim
            .session()
            .current_house()
            .and_then(|owner| sim.ready_outcome_for_owner(owner))
            .map(|outcome| outcome.kind),
        build_stamp: BUILD_STAMP.to_owned(),
    });
    snapshot.debug = Some(capture_debug(state, &runtime.simulation));
    snapshot.process = Some(capture_process(state));
    // Mean per observed frame, accumulated only while a client is watching.
    // A report point closes the window, so the brief span between that reset
    // and the next frame has nothing to report; fall back to the window the
    // point just closed rather than flashing an empty report.
    let closed = state.match_state.report_history.latest();
    let mut profile = runtime.simulation.frame_profile().report();
    if profile.as_ref().is_none_or(|report| report.frames == 0) {
        profile = closed.as_ref().and_then(|point| point.profile);
    }
    snapshot.profile = profile;
    // Render phases are process wall time, so they come from the app's meter
    // rather than the simulation's profile.
    let mut render = state.diag.render_timing.report();
    if render.as_ref().is_none_or(|report| report.frames == 0) {
        render = closed.as_ref().and_then(|point| point.render);
    }
    if render.is_none() {
        // A hidden window renders nothing, so the live meter and the latest
        // sample can both be empty; the last measured breakdown still stands.
        render = state.diag.render_report_cache;
    }
    snapshot.render = render;
    // The newest report-grade sample rides every publish; the whole ring goes
    // out once, with the history backfill.
    snapshot.report_point = state.match_state.report_history.latest();
    // Log records since the previous snapshot. Pending entries stay in the
    // bounded ring; actual evictions are reported as dropped.
    let subscribed = state
        .diag
        .telemetry_server
        .as_ref()
        .is_none_or(|server| server.has_clients());
    if subscribed && let Some(ring) = crate::util::logging::ring() {
        const MAX_PER_SNAPSHOT: usize = 64;
        let (lines, evicted) = ring.take_since_last(MAX_PER_SNAPSHOT);
        snapshot.logs = Some(crate::sim::telemetry::LogBatch {
            dropped: evicted,
            lines: lines
                .into_iter()
                .map(|line| crate::sim::telemetry::LogRecord {
                    seq: line.seq,
                    level: line.level,
                    target: line.target,
                    message: line.message,
                    t_ms: line.t_ms,
                })
                .collect(),
        });
    }
    Some(snapshot)
}

/// Process-level resources for the dashboard: the OS counters, plus the
/// adapter identity and the palette-index texture pages this process keeps
/// resident. Pure observation — nothing here touches simulation state.
fn capture_process(state: &AppState) -> ProcessStats {
    let mut stats = state.diag.process_stats.sample();
    stats.gpu_adapter = Some(bounded_text(
        &state.renderer.gpu.capture_adapter_observation().name,
        MAX_DETAIL_BYTES,
    ));
    stats.gpu_texture_mb = resident_texture_mib(state);
    stats
}

/// Resident palette-index texture bytes, in MiB.
///
/// Both atlases page palette-index (`R8Uint`, one byte per pixel) textures, so
/// a page's footprint is exactly its pixel count. This is deliberately the
/// renderer's own accounting and not a driver query: it is exact for what we
/// allocate and needs no vendor API.
fn resident_texture_mib(state: &AppState) -> f64 {
    const BYTES_PER_PIXEL: f64 = 1.0;
    let pixels = (sprite_atlas_bytes(state) + unit_atlas_bytes(state)) as f64;
    pixels * BYTES_PER_PIXEL / (1024.0 * 1024.0)
}

fn sprite_atlas_bytes(state: &AppState) -> u64 {
    state
        .match_state
        .match_presentation
        .sprite_atlas
        .as_ref()
        .map(|atlas| {
            atlas
                .pages
                .iter()
                .map(|page| u64::from(page.texture.width) * u64::from(page.texture.height))
                .sum::<u64>()
        })
        .unwrap_or(0)
}

fn unit_atlas_bytes(state: &AppState) -> u64 {
    state
        .match_state
        .match_presentation
        .unit_atlas
        .as_ref()
        .map(|atlas| {
            atlas
                .pages
                .iter()
                .map(|page| u64::from(page.texture.width) * u64::from(page.texture.height))
                .sum::<u64>()
        })
        .unwrap_or(0)
}

/// Render-loop cadence also publishes paused worlds. No snapshot work or
/// process queries occur without an authenticated subscriber.
pub fn publish_if_due(state: &mut AppState) {
    synchronize_observer(state);
    if !state.diag.telemetry_observer.active() {
        return;
    }
    let now = crate::util::time::Instant::now();
    if state
        .diag
        .telemetry_last_publish
        .is_some_and(|last| now.duration_since(last) < PUBLISH_INTERVAL)
    {
        return;
    }
    state.diag.telemetry_last_publish = Some(now);
    let Some(mut snapshot) = capture_snapshot(state) else {
        return;
    };
    // A client that attached since the last publish receives the whole-match
    // timeline once, so the time before it is not a hole. The elevation grid
    // rides the same one-shot backfill; a map load sets the dirty flag so an
    // already-attached client also receives a grid that changed under it.
    let attached = state
        .diag
        .telemetry_server
        .as_ref()
        .map(crate::app::telemetry_server::TelemetryServer::attach_generation);
    let attached_changed =
        attached.is_some_and(|attached| attached != state.diag.telemetry_history_generation);
    if (attached_changed || state.diag.telemetry_terrain_dirty) && attached.is_some() {
        if attached_changed {
            state.diag.telemetry_history_generation = attached.unwrap_or_default();
            snapshot.history = Some(state.match_state.telemetry_history.samples());
            snapshot.report_points = Some(state.match_state.report_history.points());
        }
        if state.diag.telemetry_terrain.is_none() {
            state.diag.telemetry_terrain = build_terrain_grid(state);
        }
        snapshot.terrain = state.diag.telemetry_terrain.clone();
        state.diag.telemetry_terrain_dirty = false;
    }
    match serde_json::to_string(&snapshot) {
        Ok(json) => {
            if let Some(server) = state.diag.telemetry_server.as_ref() {
                server.publish(json);
            }
        }
        Err(error) => log::warn!("telemetry publish failed: {error}"),
    }
}

/// Encode the app's elevation grid for the dashboard, or `None` when no map is
/// loaded. Row-major, two lowercase hex digits per cell, for each of the four
/// per-cell grids `TerrainGrid` carries: ground levels, land types, ramp slope
/// types and bridge deck rises. All four come off the one `ResolvedTerrainCell`
/// the map resolved, so the dashboard's water, ramps and bridges track the
/// engine's own terrain rather than a re-derivation. The grid is static for a
/// loaded map, so the caller caches the result; it is rebuilt only after a
/// world replacement.
fn build_terrain_grid(state: &AppState) -> Option<crate::sim::telemetry::TerrainGrid> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let runtime = state.match_state.sim_runtime.as_ref()?;
    let sim = &runtime.simulation;
    let width = sim.session().map_width();
    let height = sim.session().map_height();
    if width == 0 || height == 0 {
        return None;
    }
    let cells = sim.resolved_terrain();
    let stride = usize::from(width) * usize::from(height);
    let mut levels = String::with_capacity(stride * 2);
    let mut land_types = String::with_capacity(stride * 2);
    let mut slope_types = String::with_capacity(stride * 2);
    let mut deck_rises = String::with_capacity(stride * 2);
    let hex = |out: &mut String, value: u8| {
        out.push(HEX[usize::from(value >> 4)] as char);
        out.push(HEX[usize::from(value & 0x0f)] as char);
    };
    for ry in 0..height {
        for rx in 0..width {
            let cell = cells.and_then(|grid| grid.cell(rx, ry));
            // Without a resolved cell (no terrain yet) the level stays flat.
            let level = cell.map_or(0, |cell| cell.level);
            // A structural deck sits `bridge_deck_level` above the cell's own
            // ground level; a low, ground-level bridge reports no rise.
            let deck_rise = cell
                .and_then(|cell| cell.bridge_deck_level_if_any())
                .map_or(0, |deck| deck.saturating_sub(level));
            hex(&mut levels, level);
            hex(&mut land_types, cell.map_or(0, |cell| cell.land_type));
            hex(&mut slope_types, cell.map_or(0, |cell| cell.slope_type));
            hex(&mut deck_rises, deck_rise);
        }
    }
    Some(crate::sim::telemetry::TerrainGrid {
        width,
        height,
        levels,
        land_types,
        slope_types,
        deck_rises,
    })
}

/// The egui Terrain Debug panel's data, read from live cursor and selection.
fn capture_debug(state: &AppState, sim: &Simulation) -> DebugPanel {
    let (cursor_rx, cursor_ry) = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    let path_grid = sim.path_grid();
    let active_speed_type =
        crate::app::diagnostics::debug_overlays::resolve_debug_speed_type(state);

    let cursor_elevation = state
        .resolved_terrain()
        .and_then(|terrain| terrain.cell(cursor_rx, cursor_ry))
        .map_or(0, |cell| cell.level);
    let cursor_walkable = path_grid.map(|grid| grid.is_walkable(cursor_rx, cursor_ry));

    let land_rows = sim
        .resolved_terrain()
        .and_then(|terrain| terrain.cell(cursor_rx, cursor_ry))
        .map(|cell| {
            [
                (SpeedType::Foot, "Foot"),
                (SpeedType::Track, "Track"),
                (SpeedType::Wheel, "Wheel"),
                (SpeedType::Float, "Float"),
                (SpeedType::Amphibious, "Amphibious"),
            ]
            .into_iter()
            .map(|(speed_type, name)| LandRow {
                speed_type: name.to_string(),
                cost: crate::app::diagnostics::debug_overlays::land_row(cell, speed_type),
            })
            .collect()
        })
        .unwrap_or_default();

    let mut entities_at_cursor: Vec<EntityDesc> = Vec::new();
    let mut footprint: Option<String> = None;
    for entity in sim.entities().values() {
        if entity.position.rx() != cursor_rx || entity.position.ry() != cursor_ry {
            continue;
        }
        if entities_at_cursor.len() == MAX_AT_CURSOR {
            break;
        }
        entities_at_cursor.push(EntityDesc {
            type_name: bounded_text(
                sim.interner().resolve(entity.type_ref()),
                TELEMETRY_MAX_NAME_BYTES,
            ),
            category: category_label(entity.category()).to_string(),
            owner: bounded_text(
                sim.interner().resolve(entity.owner()),
                TELEMETRY_MAX_NAME_BYTES,
            ),
            hp: entity.health.current(),
            max_hp: entity_max_hp(state, sim, entity),
        });
    }
    if let Some(rules) = state.rules() {
        for entity in sim.entities().values() {
            if entity.category() != EntityCategory::Structure {
                continue;
            }
            let foundation = sim
                .interner()
                .object_type(entity.type_ref(), rules)
                .map(|obj| obj.foundation.clone())
                .unwrap_or_default();
            let (fw, fh) = foundation.dimensions();
            let (ex, ey) = (entity.position.rx(), entity.position.ry());
            if cursor_rx >= ex
                && u32::from(cursor_rx) < u32::from(ex) + u32::from(fw)
                && cursor_ry >= ey
                && u32::from(cursor_ry) < u32::from(ey) + u32::from(fh)
            {
                footprint = Some(format!(
                    "{} ({}) @ ({},{}) {}",
                    bounded_text(
                        sim.interner().resolve(entity.type_ref()),
                        TELEMETRY_MAX_NAME_BYTES
                    ),
                    // The typed Foundation owner carries its name through
                    // Display; the capture keeps the name it always showed.
                    bounded_text(&foundation.to_string(), TELEMETRY_MAX_NAME_BYTES),
                    ex,
                    ey,
                    bounded_text(
                        sim.interner().resolve(entity.owner()),
                        TELEMETRY_MAX_NAME_BYTES
                    )
                ));
                break;
            }
        }
    }

    DebugPanel {
        cursor_cell: Some((cursor_rx, cursor_ry)),
        cursor_elevation,
        grid: path_grid.map(|grid| (grid.width(), grid.height())),
        cursor_walkable,
        land_rows,
        active_speed_type: active_speed_type.name().to_string(),
        vxl_sprites: state
            .match_state
            .match_presentation
            .unit_atlas
            .as_ref()
            .map(|atlas| atlas.rendered as u32),
        entities_at_cursor,
        footprint,
        selected: capture_selection(sim),
        overlays: crate::sim::telemetry::OverlayFlags {
            terrain_costs: state.diag.debug_show_pathgrid,
            heightmap: state.diag.debug_show_heightmap,
            cell_grid: state.diag.debug_show_cell_grid,
            fog: !state.match_state.sandbox_full_visibility,
            foot_path: state.diag.debug_show_foot_path,
            paused: state.match_state.paused(),
            #[cfg(feature = "debug-tools")]
            unit_inspector: sim.debug_event_logging(),
        },
    }
}

fn capture_selection(sim: &Simulation) -> Vec<SelectedDebug> {
    sim.entities()
        .values()
        .filter(|entity| entity.selection.is_selected())
        .take(MAX_SELECTED)
        .map(|entity| capture_selected(sim, entity, sim.path_grid()))
        .collect()
}

/// The egui panel's short category labels.
fn category_label(category: EntityCategory) -> &'static str {
    match category {
        EntityCategory::Unit => "Unit",
        EntityCategory::Infantry => "Inf",
        EntityCategory::Structure => "Bld",
        EntityCategory::Aircraft => "Air",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::combat::Health;

    #[test]
    fn selection_capture_is_bounded_and_does_not_change_the_world() {
        let mut sim = Simulation::with_seed(44);
        let owner = sim.intern("Americans");
        let kind = sim.intern(&"é<&\"".repeat(300));
        for _ in 0..MAX_SELECTED + 4 {
            let id = sim.insert_synthetic_techno_for_diagnostics(
                5,
                6,
                0,
                0,
                owner,
                Health::new(100),
                kind,
                EntityCategory::Unit,
                0,
                1,
                true,
            );
            sim.entities_mut_for_test()
                .get_mut(id)
                .unwrap()
                .selection
                .set_selected_for_test(true);
        }
        let before = crate::util::save_codec::serialize(&sim).unwrap();
        let selected = capture_selection(&sim);
        assert_eq!(selected.len(), MAX_SELECTED);
        assert!(
            selected
                .iter()
                .all(|row| row.type_name.len() <= TELEMETRY_MAX_NAME_BYTES)
        );
        assert_eq!(before, crate::util::save_codec::serialize(&sim).unwrap());
        let json = serde_json::to_value(&selected).unwrap();
        #[cfg(not(feature = "debug-tools"))]
        {
            assert!(json[0].get("events").is_none());
            assert!(json[0].get("inspector_active").is_none());
        }
        #[cfg(feature = "debug-tools")]
        {
            assert_eq!(json[0]["inspector_active"], false);
            assert_eq!(json[0]["events"], serde_json::json!([]));
        }
    }
}

/// Strength from the entity's object type, or 0 when the rules are not bound.
fn entity_max_hp(
    state: &AppState,
    sim: &Simulation,
    entity: &crate::sim::game_entity::GameEntity,
) -> i32 {
    state
        .rules()
        .and_then(|rules| sim.interner().object_type(entity.type_ref(), rules))
        .map_or(0, |obj| obj.strength)
}

/// One selected entity's current identity, position, path and miner state.
fn capture_selected(
    sim: &Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
) -> SelectedDebug {
    let moving = motion_query::is_moving(entity) == Some(true);
    let (path_steps_left, path_blocked, goal, path_preview) = if moving {
        let path = entity
            .navigation
            .path_replay()
            .remaining_cells_from((entity.position.rx() as i16, entity.position.ry() as i16));
        let preview = path
            .iter()
            .take(MAX_PATH_PREVIEW)
            .enumerate()
            .map(|(index, &(px, py))| PathStep {
                cell: (px as u16, py as u16),
                walkable: path_grid.is_none_or(|grid| grid.is_walkable(px as u16, py as u16)),
                first: index == 0,
            })
            .collect();
        (
            path.len() as u32,
            entity.navigation.path_runtime().path_blocked(),
            crate::sim::movement::movement_goal_cell(entity),
            preview,
        )
    } else {
        (0, false, None, Vec::new())
    };

    let miner = entity.miner.as_ref().map(|miner| MinerDebug {
        kind: format!("{:?}", miner.kind()),
        state: format!(
            "{:?}",
            entity
                .miner_state()
                .unwrap_or(crate::sim::miner::MinerState::SearchOre)
        ),
        cargo: miner.cargo().len() as u32,
        capacity: u32::from(miner.capacity_bales()),
        stage: entity.native_stage().value(),
        rate: entity.native_stage().rate(),
        harvesting: miner.harvesting(),
        archived_ore: match entity.archive_target() {
            Some(crate::sim::combat::TargetKind::Cell(x, y)) => Some((x, y)),
            _ => None,
        },
        teleport_active: entity.teleport_state().is_some(),
    });

    SelectedDebug {
        stable_id: entity.stable_id(),
        type_name: bounded_text(
            sim.interner().resolve(entity.type_ref()),
            TELEMETRY_MAX_NAME_BYTES,
        ),
        cell: (entity.position.rx(), entity.position.ry()),
        sub: (
            entity.position.sub_x().to_num::<f32>().clamp(0.0, 255.0) as u16,
            entity.position.sub_y().to_num::<f32>().clamp(0.0, 255.0) as u16,
        ),
        moving,
        path_steps_left,
        path_blocked,
        goal,
        path_preview,
        miner,
        #[cfg(feature = "debug-tools")]
        inspector_active: entity.debug_log().is_some(),
        #[cfg(feature = "debug-tools")]
        events: entity
            .debug_log()
            .map(|log| {
                log.events()
                    .iter()
                    .rev()
                    .take(64)
                    .map(|event| crate::sim::telemetry::DebugEventReport {
                        tick: event.tick,
                        text: bounded_text(&event.kind.to_string(), 512),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}
