//! Debug visualization overlays — pathgrid walkability, land rows, etc.
//!
//! Each overlay builds `Vec<SpriteInstance>` quads drawn with the white texture
//! at low opacity using tint colors. Toggled via hotkeys (F9 = pathgrid).
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::presentation::instances::in_view;
use crate::map::resolved_terrain::ResolvedTerrainCell;
use crate::map::terrain::{self, TILE_HEIGHT, TILE_WIDTH};
use crate::render::batch::SpriteInstance;
use crate::rules::locomotor_type::SpeedType;

/// Semi-transparent tint alpha applied to debug overlay cells.
/// Multiplied into the RGB channels since the shader does `color.rgb * tint`.
const OVERLAY_ALPHA: f32 = 0.35;

/// Depth for debug overlays — drawn above terrain but below entities.
/// Between terrain (depth ~0.9+) and entities/overlays (depth ~0.0005-0.0007).
const DEBUG_OVERLAY_DEPTH: f32 = 0.0004;

// -- Terrain cost overlay colors (land row, whole percent, read capped at 100) --

/// Land row 0: impassable. Red.
const COST_BLOCKED_TINT: [f32; 3] = [OVERLAY_ALPHA, 0.0, 0.0];
/// Land row 1–65%. Orange.
const COST_VERY_ROUGH_TINT: [f32; 3] = [OVERLAY_ALPHA, 0.15, 0.0];
/// Land row 66–80%. Dark yellow.
const COST_ROUGH_TINT: [f32; 3] = [OVERLAY_ALPHA, 0.25, 0.0];
/// Land row 81–95%. Yellow.
const COST_SLOW_TINT: [f32; 3] = [OVERLAY_ALPHA, OVERLAY_ALPHA, 0.0];
/// Land row 96–100%. Green.
const COST_NORMAL_TINT: [f32; 3] = [0.0, OVERLAY_ALPHA, 0.0];

/// Map a land row to a tint color.
fn cost_to_tint(cost: u8) -> [f32; 3] {
    match cost {
        0 => COST_BLOCKED_TINT,
        1..=65 => COST_VERY_ROUGH_TINT,
        66..=80 => COST_ROUGH_TINT,
        81..=95 => COST_SLOW_TINT,
        _ => COST_NORMAL_TINT,
    }
}

/// Resolve which SpeedType the terrain cost overlay should display.
///
/// Priority: manual override → first selected entity's locomotor → Track default.
pub(crate) fn resolve_debug_speed_type(state: &AppState) -> SpeedType {
    if let Some(st) = state.diag.debug_terrain_cost_speed_type {
        return st;
    }
    state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
        .and_then(|sim| {
            sim.entities()
                .values()
                .find(|e| e.selection.is_selected())
                .and_then(|e| e.locomotor.as_ref())
                .map(|l| l.speed_type())
        })
        .unwrap_or(SpeedType::Track)
}

/// One cell's land row for `speed_type`, the whole-percent speed
/// `Can_Enter_Cell` reads. A missing row shows as 0: gamemd's table for an
/// unauthored land type is zero-filled.
pub(crate) fn land_row(cell: &ResolvedTerrainCell, speed_type: SpeedType) -> u8 {
    cell.speed_costs
        .cost_for_speed_type(speed_type)
        .unwrap_or(0)
}

/// Build terrain cost overlay instances for all visible cells.
///
/// Each cell gets a colored diamond quad based on its land row for the
/// active SpeedType: red=impassable, orange/yellow=slowed, green=full speed.
/// The row is the ground's, so a bridge cell is drawn at ground height: a
/// mover on the deck does not read it.
pub(crate) fn build_terrain_cost_overlay_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
) -> Vec<SpriteInstance> {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        log::warn!("Terrain cost overlay: no simulation");
        return Vec::new();
    };
    let speed_type = resolve_debug_speed_type(state);
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        log::warn!("Terrain cost overlay: no resolved terrain");
        return Vec::new();
    };
    let row_at = |rx: u16, ry: u16| {
        terrain
            .cell(rx, ry)
            .map_or(0, |cell| land_row(cell, speed_type))
    };

    let width: u16 = terrain.width();
    let height: u16 = terrain.height();

    // One-shot diagnostic on first call.
    use std::sync::atomic::{AtomicBool, Ordering};
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        // Sample a few cells to verify the grid has non-zero data.
        let sample_costs: Vec<u8> = (0..width.min(20)).map(|x| row_at(x, height / 2)).collect();
        log::info!(
            "Terrain cost overlay: {:?} grid {}x{}, cam=({:.0},{:.0}), screen=({:.0},{:.0}), sample costs={:?}",
            speed_type,
            width,
            height,
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
            sw,
            sh,
            sample_costs,
        );
    }

    let mut instances: Vec<SpriteInstance> = Vec::with_capacity(2048);
    let cam_x: f32 = state.match_state.input.camera_x;
    let cam_y: f32 = state.match_state.input.camera_y;

    for ry in 0..height {
        for rx in 0..width {
            let z: u8 = state.height_map().get(&(rx, ry)).copied().unwrap_or(0);
            let (sx, sy) = terrain::iso_to_screen(rx, ry, z);

            if !in_view(sx, sy, TILE_WIDTH, TILE_HEIGHT, cam_x, cam_y, sw, sh, 60.0) {
                continue;
            }

            let cost: u8 = row_at(rx, ry);
            let tint: [f32; 3] = cost_to_tint(cost);

            instances.push(SpriteInstance {
                position: [sx, sy],
                size: [TILE_WIDTH, TILE_HEIGHT],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: DEBUG_OVERLAY_DEPTH,
                tint,
                alpha: 1.0,
                ..Default::default()
            });
        }
    }

    instances
}

/// Build debug height map overlay — each cell tinted by elevation.
/// Ground cells use white brightness proportional to z level.
/// Bridge deck cells use blue tint to distinguish them from ground.
pub(crate) fn build_heightmap_overlay_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
) -> Vec<SpriteInstance> {
    if state.height_map().is_empty() {
        return Vec::new();
    }
    let cam_x: f32 = state.match_state.input.camera_x;
    let cam_y: f32 = state.match_state.input.camera_y;
    let mut instances: Vec<SpriteInstance> = Vec::with_capacity(2048);

    // Find max z for normalization.
    let max_z: u8 = state
        .height_map()
        .values()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);

    // One-shot diagnostic: log bridge_height_map size on first call.
    use std::sync::atomic::{AtomicBool, Ordering};
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        let bridge_count = state.bridge_height_map().len();
        let sample: Vec<_> = state.bridge_height_map().iter().take(10).collect();
        log::info!(
            "HeightmapOverlay: bridge_height_map has {} entries, height_map has {} entries. Sample: {:?}",
            bridge_count,
            state.height_map().len(),
            sample,
        );
    }

    for (&(rx, ry), &z) in state.height_map() {
        // Bridge cells: render at deck height so the overlay aligns with the bridge surface.
        let render_z: u8 = state
            .bridge_height_map()
            .get(&(rx, ry))
            .copied()
            .unwrap_or(z);
        let (sx, sy) = terrain::iso_to_screen(rx, ry, render_z);
        if !in_view(sx, sy, TILE_WIDTH, TILE_HEIGHT, cam_x, cam_y, sw, sh, 60.0) {
            continue;
        }

        let is_bridge = state.bridge_height_map().contains_key(&(rx, ry));
        let tint: [f32; 3] = if is_bridge {
            // Solid blue for bridge deck cells — high alpha so the bridge
            // structure doesn't show through the overlay.
            [0.0, 0.15, 0.7]
        } else {
            // White/grey brightness by elevation.
            let intensity: f32 = (z as f32 / max_z as f32) * OVERLAY_ALPHA;
            [intensity, intensity, intensity]
        };

        // Bridge cells need a lower depth to avoid being occluded by bridge
        // overlay sprites that write to the depth buffer in the entity pass.
        let cell_depth: f32 = if is_bridge {
            0.0001
        } else {
            DEBUG_OVERLAY_DEPTH
        };
        instances.push(SpriteInstance {
            position: [sx, sy],
            size: [TILE_WIDTH, TILE_HEIGHT],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth: cell_depth,
            tint,
            alpha: 1.0,
            ..Default::default()
        });
    }
    instances
}

/// Build debug cell grid overlay — diamond outlines for terrain cells (blue)
/// and overlay cells (yellow) so alignment between the two can be verified visually.
///
/// Both use `iso_to_screen()` with the same height map. If they misalign,
/// the outlines will visually separate — making coordinate bugs obvious.
pub(crate) fn build_cell_grid_overlay_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
) -> Vec<SpriteInstance> {
    let mut instances: Vec<SpriteInstance> = Vec::with_capacity(4096);
    let cam_x: f32 = state.match_state.input.camera_x;
    let cam_y: f32 = state.match_state.input.camera_y;

    /// Terrain cell grid: bright cyan outlines.
    const TERRAIN_TINT: [f32; 3] = [0.0, 0.9, 0.9];
    /// Overlay cell grid: bright yellow outlines.
    const OVERLAY_TINT: [f32; 3] = [1.0, 1.0, 0.0];
    /// Depth — just above terrain but below entities.
    const GRID_DEPTH: f32 = 0.0003;

    // Terrain grid cells.
    if let Some(grid) = &state.match_state.match_presentation.terrain_grid {
        for cell in &grid.cells {
            if !in_view(
                cell.screen_x,
                cell.screen_y,
                TILE_WIDTH,
                TILE_HEIGHT,
                cam_x,
                cam_y,
                sw,
                sh,
                60.0,
            ) {
                continue;
            }
            instances.push(SpriteInstance {
                position: [cell.screen_x, cell.screen_y],
                size: [TILE_WIDTH, TILE_HEIGHT],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: GRID_DEPTH,
                tint: TERRAIN_TINT,
                alpha: 1.0,
                ..Default::default()
            });
        }
    }

    // Overlay cells — re-compute iso_to_screen for each overlay entry so the
    // diamond outline uses the exact same formula as overlay rendering.
    for entry in state.match_state.match_presentation.overlays.iter() {
        let z: u8 = state
            .height_map()
            .get(&(entry.rx, entry.ry))
            .copied()
            .unwrap_or(0);
        let (sx, sy) = terrain::iso_to_screen(entry.rx, entry.ry, z);
        if !in_view(sx, sy, TILE_WIDTH, TILE_HEIGHT, cam_x, cam_y, sw, sh, 60.0) {
            continue;
        }
        instances.push(SpriteInstance {
            position: [sx, sy],
            size: [TILE_WIDTH, TILE_HEIGHT],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth: GRID_DEPTH - 0.0001,
            tint: OVERLAY_TINT,
            alpha: 1.0,
            ..Default::default()
        });
    }

    instances
}

/// Build path visualization overlay for selected entities.
///
/// Draws the current computed path as colored diamond cells:
/// - Cyan: current/upcoming path step
/// - Magenta: next step (immediate target)
/// - Yellow: final goal
pub(crate) fn build_path_overlay_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
) -> Vec<SpriteInstance> {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return Vec::new();
    };
    let cam_x: f32 = state.match_state.input.camera_x;
    let cam_y: f32 = state.match_state.input.camera_y;
    let mut instances: Vec<SpriteInstance> = Vec::with_capacity(128);

    /// Path step color: bright cyan.
    const PATH_TINT: [f32; 3] = [0.0, 0.6, 0.6];
    /// Next immediate step: bright magenta.
    const NEXT_TINT: [f32; 3] = [0.7, 0.0, 0.7];
    /// Final goal: bright yellow.
    const GOAL_TINT: [f32; 3] = [0.7, 0.7, 0.0];
    const PATH_DEPTH: f32 = 0.0002;

    for entity in sim.entities().values() {
        if !entity.selection.is_selected() {
            continue;
        }
        if crate::sim::movement::motion_query::is_moving(entity) != Some(true) {
            continue;
        }
        // The unpaid Foot+5E0 words, walked from the mover's live cell.
        let path: Vec<(u16, u16)> = entity
            .navigation
            .path_replay
            .remaining_cells_from((entity.position.rx() as i16, entity.position.ry() as i16))
            .into_iter()
            .map(|(x, y)| (x as u16, y as u16))
            .collect();
        for (i, &(px, py)) in path.iter().enumerate() {
            let z: u8 = state.height_map().get(&(px, py)).copied().unwrap_or(0);
            let (sx, sy) = terrain::iso_to_screen(px, py, z);
            if !in_view(sx, sy, TILE_WIDTH, TILE_HEIGHT, cam_x, cam_y, sw, sh, 60.0) {
                continue;
            }
            let tint: [f32; 3] = if i == 0 {
                NEXT_TINT
            } else if i == path.len() - 1 {
                GOAL_TINT
            } else {
                PATH_TINT
            };
            instances.push(SpriteInstance {
                position: [sx, sy],
                size: [TILE_WIDTH, TILE_HEIGHT],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: PATH_DEPTH,
                tint,
                alpha: 1.0,
                ..Default::default()
            });
        }
        // Also draw the owner's goal if it's not the last path step.
        if let Some(goal) = crate::sim::movement::movement_goal_cell(entity) {
            let last = path.last().copied().unwrap_or((0, 0));
            if goal != last {
                let z: u8 = state
                    .height_map()
                    .get(&(goal.0, goal.1))
                    .copied()
                    .unwrap_or(0);
                let (sx, sy) = terrain::iso_to_screen(goal.0, goal.1, z);
                if in_view(sx, sy, TILE_WIDTH, TILE_HEIGHT, cam_x, cam_y, sw, sh, 60.0) {
                    instances.push(SpriteInstance {
                        position: [sx, sy],
                        size: [TILE_WIDTH, TILE_HEIGHT],
                        uv_origin: [0.0, 0.0],
                        uv_size: [1.0, 1.0],
                        depth: PATH_DEPTH,
                        tint: GOAL_TINT,
                        alpha: 1.0,
                        ..Default::default()
                    });
                }
            }
        }
    }

    instances
}

// -- Foot-path polyline overlay (Ctrl+Shift+O) --

/// The route line: the same blue as the reference debug path.
const PATH_LINE_TINT: [f32; 3] = [0.25, 0.44, 0.91];
const PATH_LINE_ALPHA: f32 = 0.95;
/// The route stroke, rasterised the way the rest of the renderer draws lines.
///
/// This renderer has no rotated-quad concept — the sprite vertex shader only
/// does `position + local * size`, and the one real line path
/// (`render::terrain_surface_lines` + `surface_line::rasterize`) is a CPU DDA
/// walk emitting one quad per pixel. The stroke follows that idiom: step one
/// world pixel along the segment and drop a quad there. Spaced blobs along a
/// diagonal read as a staircase.
///
/// The quad is wider than the step so consecutive pixels overlap and leave no
/// seam. 1.5px measured as a 2px line that was hard to see against terrain; 7px
/// fused into a ~9px band.
const LINE_STEP_PX: f32 = 1.0;
const LINE_PIXEL_SIZE: f32 = 2.5;
/// Waypoint node, drawn over the stroke as an iso diamond: nodes mark *cells*,
/// so the cell texture is right for them — the stroke must not use it, or its
/// transparent corners make the line faint.
const NODE_SIZE: f32 = 8.0;
/// Immediate next step: bright magenta.
const PATH_NEXT_TINT: [f32; 3] = [0.91, 0.20, 0.91];
/// Final step / arrival cell: bright yellow.
const PATH_GOAL_TINT: [f32; 3] = [0.98, 0.80, 0.08];
/// Step-index label tint. Near-white with a blue cast: a mid-blue was too low
/// a contrast against lit terrain to read at a glance.
const PATH_LABEL_TINT: [f32; 3] = [0.90, 0.96, 1.0];
const PATH_LABEL_HALO_TINT: [f32; 3] = [0.01, 0.02, 0.09];
/// Depth within the debug pass. The debug batches draw with no depth writes, so
/// what orders them against each other is submission order, not this value; it
/// matches the other route overlays so a shared batch keeps its own ordering.
const FOOT_PATH_DEPTH: f32 = 0.00025;
/// Label scale for `BitFont::build_text`. GAME.FNT glyphs are 16 rows tall, so
/// this lands a digit at its native 16px. 0.5 (~8px) measured as unreadable
/// across a whole route and 0.8 (~13px) was the old value.
const PATH_LABEL_SCALE: f32 = 1.0;
/// At most this many step indices are labelled. A long route would otherwise
/// bury the map in digits; past the cap the ends are always labelled and the
/// rest is sampled at `path_label_stride`.
const MAX_PATH_LABELS: usize = 32;

/// Camera offset for `BitFont::build_text` on the **world** camera path.
///
/// Must stay zero. `build_text` adds this to every glyph, and these batches are
/// drawn with the world camera still bound in group 0, so the shader already
/// subtracts it — passing the live camera double-counts the scroll and throws
/// every digit off-screen. Only the UI-camera path (`draw_pooled_ui`, used by
/// tooltips and messages) needs the camera added back.
const NO_CAMERA_OFFSET: [f32; 2] = [0.0, 0.0];

/// Screen centre of a map cell, given `iso_to_screen`'s top-left origin.
fn cell_center(sx: f32, sy: f32) -> [f32; 2] {
    [sx + TILE_WIDTH / 2.0, sy + TILE_HEIGHT / 2.0]
}

/// The halo/digit submission order, as the drawing loop uses it.
///
/// Instances of one texture do not blend against each other — later quads
/// overwrite earlier ones, so the dark halo copies must be pushed before the
/// digit or they bury the glyph they outline. Data rather than inline code so a
/// test can pin the order.
const LABEL_PASSES: [(f32, f32, [f32; 3]); 5] = [
    (-1.0, 0.0, PATH_LABEL_HALO_TINT),
    (1.0, 0.0, PATH_LABEL_HALO_TINT),
    (0.0, -1.0, PATH_LABEL_HALO_TINT),
    (0.0, 1.0, PATH_LABEL_HALO_TINT),
    (0.0, 0.0, PATH_LABEL_TINT),
];

/// The viewport a stroke is culled against, in screen pixels: the camera is the
/// viewport's top-left corner, as every `in_view` caller here assumes.
struct FootPathView {
    cam_x: f32,
    cam_y: f32,
    sw: f32,
    sh: f32,
}

/// Rasterise the stroke covering segment `a -> b`, excluding the endpoint (the
/// caller's next segment or the node ring draws it) so joints never double up.
///
/// A long route would otherwise emit thousands of quads for pixels nobody can
/// see, so a segment whose bounding box misses the viewport is dropped whole and
/// the rest culls per pixel.
fn push_stroke_along(
    a: [f32; 2],
    b: [f32; 2],
    tint: [f32; 3],
    view: &FootPathView,
    instances: &mut Vec<SpriteInstance>,
) {
    let delta = [b[0] - a[0], b[1] - a[1]];
    let dist = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
    if dist <= 0.0 {
        return;
    }
    if !in_view(
        a[0].min(b[0]) - LINE_PIXEL_SIZE,
        a[1].min(b[1]) - LINE_PIXEL_SIZE,
        delta[0].abs() + 2.0 * LINE_PIXEL_SIZE,
        delta[1].abs() + 2.0 * LINE_PIXEL_SIZE,
        view.cam_x,
        view.cam_y,
        view.sw,
        view.sh,
        0.0,
    ) {
        return;
    }
    // One step per pixel, minus the endpoint.
    let steps = ((dist / LINE_STEP_PX).ceil() as usize).max(1);
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let p = [a[0] + delta[0] * t, a[1] + delta[1] * t];
        let origin = [p[0] - LINE_PIXEL_SIZE / 2.0, p[1] - LINE_PIXEL_SIZE / 2.0];
        if !in_view(
            origin[0],
            origin[1],
            LINE_PIXEL_SIZE,
            LINE_PIXEL_SIZE,
            view.cam_x,
            view.cam_y,
            view.sw,
            view.sh,
            0.0,
        ) {
            continue;
        }
        instances.push(SpriteInstance {
            position: origin,
            size: [LINE_PIXEL_SIZE, LINE_PIXEL_SIZE],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth: FOOT_PATH_DEPTH,
            tint,
            alpha: PATH_LINE_ALPHA,
            ..Default::default()
        });
    }
}

/// One waypoint node centred on a screen point.
fn push_node(p: [f32; 2], tint: [f32; 3], instances: &mut Vec<SpriteInstance>) {
    instances.push(SpriteInstance {
        position: [p[0] - NODE_SIZE / 2.0, p[1] - NODE_SIZE / 2.0],
        size: [NODE_SIZE, NODE_SIZE],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        depth: FOOT_PATH_DEPTH,
        tint,
        alpha: 1.0,
        ..Default::default()
    });
}

/// How many steps to skip between printed step indices on a route of `len`
/// steps: 1 labels every step, and longer routes thin out to an even sample so
/// the digit count stays bounded. Always at least 1 (never a divide-by-zero).
fn path_label_stride(len: usize) -> usize {
    if len <= MAX_PATH_LABELS {
        1
    } else {
        len.div_ceil(MAX_PATH_LABELS)
    }
}

/// The foot-path overlay's instance vectors, one per bound texture: a batch
/// binds exactly one texture, so the white stroke, the diamond nodes and the
/// GAME.FNT glyphs cannot share one.
pub(crate) struct FootPathInstances {
    /// Route stroke (solid white texture).
    pub stroke: Vec<SpriteInstance>,
    /// Waypoint nodes (filled-diamond texture).
    pub nodes: Vec<SpriteInstance>,
    /// Step-index glyphs (GAME.FNT atlas).
    pub text: Vec<SpriteInstance>,
}

/// One request to print a step index at a waypoint cell, whose centre is given.
type FootPathLabel = ((u16, u16), usize, [f32; 2]);

/// Resolve which step indices to print and where.
///
/// Selected units following the same route ask for the same digits at the same
/// cell, so an exact `(cell, step)` pair is printed once. The distinct steps
/// that meet at one cell print as one comma-separated string: two digits side by
/// side read as a single larger number. Sorting the requests keeps the layout
/// independent of entity iteration order.
fn plan_foot_path_labels(mut labels: Vec<FootPathLabel>) -> Vec<(Vec<usize>, [f32; 2])> {
    labels.sort_unstable_by_key(|(cell, step, _)| (*cell, *step));
    labels.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    let mut planned: Vec<(Vec<usize>, [f32; 2])> = Vec::new();
    let mut previous_cell: Option<(u16, u16)> = None;
    for (cell, step, point) in labels {
        if previous_cell != Some(cell) {
            // Clear of the node ring, above its centre.
            planned.push((
                Vec::new(),
                [point[0] + NODE_SIZE / 2.0, point[1] - NODE_SIZE / 2.0],
            ));
            previous_cell = Some(cell);
        }
        planned.last_mut().expect("one group per cell").0.push(step);
    }
    planned
}

/// The text printed at one waypoint: its step indices, comma-separated.
fn foot_path_label_text(steps: &[usize]) -> String {
    steps
        .iter()
        .map(|step| step.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Build the numbered foot-path polyline overlay for selected entities.
///
/// Draws each selected unit's unpaid route (`navigation.path_replay`, the
/// Foot+5E0 words every locomotor shares, walked from its live cell) as a
/// continuous blue line through the cell centres with the step index printed at
/// every waypoint, so both the shape of the route and the order of its steps
/// read at a glance. The immediate next step is magenta and the arrival cell
/// yellow, and the owner's goal is included when it sits past the last unpaid
/// cell.
///
/// Rebuilt every frame, so the line shortens behind the unit as it walks.
pub(crate) fn build_foot_path_overlay_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
) -> FootPathInstances {
    let mut stroke: Vec<SpriteInstance> = Vec::with_capacity(1024);
    let mut nodes: Vec<SpriteInstance> = Vec::with_capacity(64);
    let mut text: Vec<SpriteInstance> = Vec::with_capacity(128);
    let mut labels: Vec<FootPathLabel> = Vec::new();
    let view = FootPathView {
        cam_x: state.match_state.input.camera_x,
        cam_y: state.match_state.input.camera_y,
        sw,
        sh,
    };
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return FootPathInstances {
            stroke,
            nodes,
            text,
        };
    };

    for entity in sim.entities().values() {
        if !entity.selection.is_selected() {
            continue;
        }
        if crate::sim::movement::motion_query::is_moving(entity) != Some(true) {
            continue;
        }
        // The unpaid Foot+5E0 words, walked from the mover's live cell.
        let mut path: Vec<(u16, u16)> = entity
            .navigation
            .path_replay
            .remaining_cells_from((entity.position.rx() as i16, entity.position.ry() as i16))
            .into_iter()
            .map(|(x, y)| (x as u16, y as u16))
            .collect();
        if let Some(goal) = crate::sim::movement::movement_goal_cell(entity)
            .filter(|goal| path.last() != Some(goal))
        {
            path.push(goal);
        }
        if path.is_empty() {
            continue;
        }

        // Waypoints keep their cells so units sharing a cell share a label.
        let mut points: Vec<((u16, u16), [f32; 2])> = Vec::with_capacity(path.len());
        for &(px, py) in &path {
            let z: u8 = state.height_map().get(&(px, py)).copied().unwrap_or(0);
            let (sx, sy) = terrain::iso_to_screen(px, py, z);
            points.push(((px, py), cell_center(sx, sy)));
        }

        for pair in points.windows(2) {
            push_stroke_along(pair[0].1, pair[1].1, PATH_LINE_TINT, &view, &mut stroke);
        }

        // Node ring + step index requests. Index 1 is the immediate next step
        // and the last index is the arrival cell, matching the reference
        // numbering.
        let len = points.len();
        let stride = path_label_stride(len);
        for (i, (cell, p)) in points.iter().enumerate() {
            let is_next = i == 0;
            let is_goal = i + 1 == len;
            push_node(
                *p,
                if is_goal {
                    PATH_GOAL_TINT
                } else if is_next {
                    PATH_NEXT_TINT
                } else {
                    PATH_LINE_TINT
                },
                &mut nodes,
            );
            // Label the ends always; sample the middle on long routes.
            if !(is_next || is_goal || (i + 1) % stride == 0) {
                continue;
            }
            labels.push((*cell, i + 1, *p));
        }
    }

    // One string per waypoint: the steps that meet there comma-separated, so a
    // convoy crossing at different step numbers never reads as one big number.
    for (steps, origin) in plan_foot_path_labels(labels) {
        let label = foot_path_label_text(&steps);
        // Halo first, digit on top: a 1px ring in four directions keeps the
        // number readable over lit and shadowed terrain without a box. See
        // `LABEL_PASSES` for why the order is load-bearing.
        for (dx, dy, tint) in LABEL_PASSES {
            text.extend(state.renderer.bit_font.build_text(
                &label,
                origin[0] + dx,
                origin[1] + dy,
                PATH_LABEL_SCALE,
                FOOT_PATH_DEPTH,
                tint,
                NO_CAMERA_OFFSET,
            ));
        }
    }

    FootPathInstances {
        stroke,
        nodes,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A viewport so large that nothing the stroke tests draw can leave it, so
    /// the rasterising tests read as before culling existed.
    fn wide_view() -> FootPathView {
        FootPathView {
            cam_x: -100_000.0,
            cam_y: -100_000.0,
            sw: 200_000.0,
            sh: 200_000.0,
        }
    }

    #[test]
    fn stroke_rasterises_one_quad_per_pixel_and_stops_short_of_the_endpoint() {
        // The stroke follows the project's line idiom (CPU walk, one quad per
        // pixel) rather than spaced blobs. A 30px horizontal segment must
        // therefore produce ~30 quads, and must not emit `b` itself — the next
        // segment (or the node ring) owns that pixel.
        let mut out = Vec::new();
        push_stroke_along(
            [0.0, 0.0],
            [30.0, 0.0],
            PATH_LINE_TINT,
            &wide_view(),
            &mut out,
        );
        assert_eq!(out.len(), 30);
        let last = out.last().unwrap();
        assert!(last.position[0] < 30.0 - LINE_PIXEL_SIZE / 2.0);
        assert_eq!(out[0].tint, PATH_LINE_TINT);
        assert_eq!(out[0].alpha, PATH_LINE_ALPHA);
    }

    #[test]
    fn stroke_ignores_a_degenerate_zero_length_segment() {
        // Two coincident waypoints must not loop forever or emit NaN.
        let mut out = Vec::new();
        push_stroke_along(
            [5.0, 5.0],
            [5.0, 5.0],
            PATH_LINE_TINT,
            &wide_view(),
            &mut out,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn stroke_covers_a_diagonal_without_gaps() {
        // The regression this replaces: a sparse chain along a diagonal read as
        // a visible staircase. One quad per pixel along the hypotenuse gives
        // as many quads as the segment is long.
        let mut out = Vec::new();
        push_stroke_along(
            [0.0, 0.0],
            [30.0, 40.0],
            PATH_LINE_TINT,
            &wide_view(),
            &mut out,
        );
        let len = (30.0f32 * 30.0 + 40.0 * 40.0).sqrt();
        assert_eq!(out.len(), len.ceil() as usize);
    }

    #[test]
    fn stroke_drops_a_segment_whose_box_misses_the_viewport() {
        // The cost this removes: a route across a large map walked every world
        // pixel of every segment, off-screen included.
        let view = FootPathView {
            cam_x: 0.0,
            cam_y: 0.0,
            sw: 640.0,
            sh: 480.0,
        };
        let mut out = Vec::new();
        push_stroke_along(
            [5_000.0, 5_000.0],
            [5_400.0, 5_000.0],
            PATH_LINE_TINT,
            &view,
            &mut out,
        );
        assert!(out.is_empty(), "a segment far outside adds no instances");
    }

    #[test]
    fn label_digit_is_submitted_after_every_halo_copy() {
        // Regression guard: submitting the digit first let the four dark halo
        // copies paint over it, so every step index rendered as a black blob
        // (2042 dark px against 282 light px in the screenshot that caught it).
        let digit = LABEL_PASSES
            .iter()
            .position(|(_, _, tint)| *tint == PATH_LABEL_TINT)
            .expect("digit pass present");
        assert_eq!(digit, LABEL_PASSES.len() - 1, "digit must be last");
        for tint in LABEL_PASSES.iter().take(digit).map(|(_, _, tint)| tint) {
            assert_eq!(
                *tint, PATH_LABEL_HALO_TINT,
                "every pass before the digit must be halo"
            );
        }
    }

    fn plan(labels: Vec<FootPathLabel>) -> Vec<(Vec<usize>, [f32; 2])> {
        plan_foot_path_labels(labels)
    }

    /// The label anchor: clear of the node ring, above its centre.
    fn anchor(point: [f32; 2]) -> [f32; 2] {
        [point[0] + NODE_SIZE / 2.0, point[1] - NODE_SIZE / 2.0]
    }

    #[test]
    fn two_units_sharing_a_waypoint_print_the_step_once() {
        // The overlap this replaces: a convoy down one corridor asked for the
        // same digit at the same cell once per unit, stacking identical glyphs.
        let out = plan(vec![
            ((5, 5), 3, [100.0, 200.0]),
            ((5, 5), 3, [100.0, 200.0]),
            ((5, 5), 3, [100.0, 200.0]),
        ]);
        assert_eq!(out, vec![(vec![3], anchor([100.0, 200.0]))]);
    }

    #[test]
    fn label_plan_does_not_depend_on_entity_iteration_order() {
        let a = plan(vec![
            ((9, 9), 1, [10.0, 10.0]),
            ((9, 9), 4, [10.0, 10.0]),
            ((2, 3), 1, [70.0, 70.0]),
        ]);
        let b = plan(vec![
            ((2, 3), 1, [70.0, 70.0]),
            ((9, 9), 4, [10.0, 10.0]),
            ((9, 9), 1, [10.0, 10.0]),
        ]);
        assert_eq!(a, b);
    }
}
