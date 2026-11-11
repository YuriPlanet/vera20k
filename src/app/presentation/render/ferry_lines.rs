//! VERA ferry order (`app::input::ferry`): its circles and waypoint paths as
//! world-surface stores. The ferry owns what is shown; `planning_lines` owns
//! the stroke and marker stores this lowering goes through.

use super::planning_lines::{
    emit_marker, emit_marker_outer, emit_segment, emit_solid_segment, project, stroke_phase,
    stroke_rgb,
};
use crate::app::AppState;
use crate::app::presentation::target_lines::TacticalViewport;
use crate::render::batch::SpriteInstance;
use crate::sim::components::DriveCoord;

/// Where a transport loads, where it unloads, and an armed click that would
/// be refused.
const LOAD_RGB: [u8; 3] = [64, 153, 255];
const UNLOAD_RGB: [u8; 3] = [255, 230, 38];
const REFUSED_RGB: [u8; 3] = [255, 64, 51];

/// The pick-up and drop circles, each a closed one-pixel solid stroke. They
/// are for the ground pass (`draw_passes`, "ground_marks"): drawn on the
/// finished ground and before the buildings and objects, which cover them.
pub(super) fn build_rings(state: &AppState, view: TacticalViewport) -> Vec<SpriteInstance> {
    let mut out = Vec::new();
    for ring in crate::app::input::ferry::marks(state) {
        let rgb = if ring.refused {
            REFUSED_RGB
        } else if ring.drop {
            UNLOAD_RGB
        } else {
            LOAD_RGB
        };
        let points: Vec<[i32; 2]> = ring
            .points
            .iter()
            .map(|&coord| project(coord, view))
            .collect();
        for (index, &from) in points.iter().enumerate() {
            let to = points[(index + 1) % points.len()];
            emit_solid_segment(&mut out, from, to, rgb, 1, view);
        }
    }
    out
}

/// The strokes and markers of the waypoint paths, each once: transports sent
/// together share their waypoints. Each entry carries whether its path is
/// still being clicked.
#[derive(Debug, Default)]
struct PathStrokes {
    lines: Vec<([i32; 2], [i32; 2], bool)>,
    /// Where a path starts: its pick-up spot.
    starts: Vec<([i32; 2], bool)>,
    /// Every later point: the waypoints and the drop spot.
    nodes: Vec<([i32; 2], bool)>,
}

impl PathStrokes {
    fn add(&mut self, points: &[[i32; 2]], pending: bool) {
        fn once<T: PartialEq>(list: &mut Vec<T>, item: T) {
            if !list.contains(&item) {
                list.push(item);
            }
        }
        let Some((&start, rest)) = points.split_first() else {
            return;
        };
        once(&mut self.starts, (start, pending));
        let mut from = start;
        for &to in rest {
            once(&mut self.lines, (from, to, pending));
            once(&mut self.nodes, (to, pending));
            from = to;
        }
    }
}

/// A selected transport's waypoints in Planning Mode's look, in its stores'
/// order: every outer stroke and marker first (63B0A0), then the patterned
/// strokes and filled markers (63B150). A path starts as an actor's
/// connection does (6x6 outline, 4x4 fill, 63BE60) and every later point is a
/// node (8x8 outline, 6x6 fill, 63CAE0). The outer colour is a selected
/// member's white (63D0F0), since only a selected transport shows its path;
/// waypoints still being clicked take the bright colour of nodes not yet
/// committed. Unlike Planning Mode's own lines these show without the mode:
/// the ferry keeps its own waypoints.
pub(super) fn build_paths(
    state: &AppState,
    view: TacticalViewport,
    wall_ms: u32,
) -> Vec<SpriteInstance> {
    let mut out = Vec::new();
    let paths = crate::app::input::ferry::waypoint_paths(state);
    if paths.is_empty() {
        return out;
    }
    let Some(rt) = state.match_state.sim_runtime.presentation() else {
        return out;
    };
    let sim = &rt.simulation;
    let Some(viewer) = sim.session().current_house() else {
        return out;
    };
    let cells = crate::app::match_runtime::sim_tick::tactical_cells(sim);
    let mut strokes = PathStrokes::default();
    for path in &paths {
        let points: Vec<[i32; 2]> = path
            .cells
            .iter()
            .map(|&(rx, ry)| {
                let level = cells
                    .and_then(|cells| cells.cell_level(rx, ry))
                    .unwrap_or(0);
                let z = i32::from(level) * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;
                project(DriveCoord::cell(rx, ry, z), view)
            })
            .collect();
        strokes.add(&points, path.pending);
    }
    let phase = stroke_phase(wall_ms);
    let color = |pending| stroke_rgb(state, sim, &rt.resources.rules, viewer, pending);
    let white = [255; 3];
    for &(from, to, _) in &strokes.lines {
        emit_solid_segment(&mut out, from, to, white, 4, view);
    }
    for &(at, _) in &strokes.nodes {
        emit_marker_outer(&mut out, at, 8, white, view);
    }
    for &(at, _) in &strokes.starts {
        emit_marker_outer(&mut out, at, 6, white, view);
    }
    for &(from, to, pending) in &strokes.lines {
        emit_segment(&mut out, from, to, color(pending), phase, 2, view);
    }
    for &(at, pending) in &strokes.nodes {
        emit_marker(&mut out, at, 6, color(pending), view);
    }
    for &(at, pending) in &strokes.starts {
        emit_marker(&mut out, at, 4, color(pending), view);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_that_share_waypoints_draw_each_stroke_once() {
        // Two transports sent together: their own pick-up spots, the same
        // waypoint and drop spot.
        let mut strokes = PathStrokes::default();
        strokes.add(&[[0, 0], [40, 10], [80, 10]], false);
        strokes.add(&[[0, 20], [40, 10], [80, 10]], false);
        assert_eq!(strokes.starts, [([0, 0], false), ([0, 20], false)]);
        assert_eq!(strokes.nodes, [([40, 10], false), ([80, 10], false)]);
        assert_eq!(
            strokes.lines,
            [
                ([0, 0], [40, 10], false),
                ([40, 10], [80, 10], false),
                ([0, 20], [40, 10], false),
            ]
        );
    }
}
