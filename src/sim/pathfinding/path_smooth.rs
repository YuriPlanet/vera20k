//! Path smoothing — post-processes raw A* paths for natural-looking movement.
//!
//! `AStar_main_loop` @ `0x00429A90` runs two passes on every successful search:
//! `Path_smooth_corners` @ `0x0042B210` (call at `0x0042A415`) then
//! `Path_optimize_straight_segments` @ `0x0042B7F0` (call at `0x0042A41E`).
//!
//! **Pass 1 — Zigzag smoothing**: Replaces 90-degree zigzag pairs (e.g. N then E)
//! with a single diagonal shortcut (NE), if the shortcut cell is walkable and
//! diagonal corner-cutting rules are satisfied.
//!
//! Ported from `Path_smooth_corners` / `Path_smooth_single_segment` @
//! `0x0042B420`, but **not** completely: native rejects a replacement cell when
//! any of three terms holds — the `+0x1AC` cell predicate, the `0x40000` search
//! marker, or `Get_Slope_Cost_At_Cell(coord, this[0x87]) ×
//! FootClass::Get_Slope_Speed_Factor() >= 1.0` — and carries a per-step z with a
//! bridge lift (`if (z_prev - z == 4 && flags & 0x100) z += 4`) into that
//! predicate. VERA validates with its `walkable` closure alone and carries no z.
//! **VERA-internal, gamemd equivalent UNCHECKED.** Trigger: any smoothing
//! candidate on sloped ground or across a bridge step. Player effect: VERA
//! collapses a zigzag onto a slope retail routes around, and can smooth across a
//! height step native keeps separate. Frequency: pass 1 runs on every successful
//! A*, and slope terrain is on most retail maps. Downstream risk: the slope term
//! needs two native helpers VERA does not have yet.
//!
//! **Pass 2 — absent.** Native's `Path_optimize_straight_segments` @ `0x0042B7F0`
//! scans a 20-step window, closes a segment when the running `|dx|` or `|dy|`
//! *decreases* (an axis reversal) and otherwise reroutes when the Chebyshev
//! displacement from the segment origin fails to exceed a stored high-water
//! mark; its reroute writes `0xFFFFFFFE` delete markers and compacts, so the
//! native pass **shortens** the path. Trigger: every successful A*. Player
//! effect: VERA keeps path steps retail removes. Frequency: every path, every
//! match. Downstream risk: low until ported; a former drift-correction stand-in
//! here never fired (its cross product was identically zero) and was removed.
//!
//! ## Dependency rules
//! - Part of sim/ — depends only on sim/locomotor (MovementLayer).
//! - Walkability is injected via closures, no direct grid dependency.

use crate::sim::movement::locomotor::MovementLayer;

// ---------------------------------------------------------------------------
// Direction utilities
// ---------------------------------------------------------------------------

/// Direction index: 0–7 = compass directions matching pathfinding::NEIGHBORS order.
/// N=0, NE=1, E=2, SE=3, S=4, SW=5, W=6, NW=7.
// Direction index is u8: 0–7 compass, 255 = invalid/deleted.

/// Sentinel for deleted/invalid path entries.
const DIR_INVALID: u8 = 255;

/// Delta table: direction index → (dx, dy). Same order as pathfinding::NEIGHBORS.
const DIR_DELTAS: [(i32, i32); 8] = crate::util::direction::DIRECTION_DELTAS;

/// Returns which of the 8 compass directions connects two adjacent cells,
/// or `DIR_INVALID` if the cells are not 8-connected neighbors.
fn direction_between(from: (u16, u16), to: (u16, u16)) -> u8 {
    let dx = to.0 as i32 - from.0 as i32;
    let dy = to.1 as i32 - from.1 as i32;
    for (i, &(ddx, ddy)) in DIR_DELTAS.iter().enumerate() {
        if dx == ddx && dy == ddy {
            return i as u8;
        }
    }
    DIR_INVALID
}

/// Minimum angular distance between two directions on the 8-direction wheel.
/// Returns 0–4 (0 = same direction, 4 = opposite). Pass 1 uses the native
/// `(new - anchor) & 7` form directly; this stays as a test-side reference.
#[cfg(test)]
fn dir_diff(a: u8, b: u8) -> u8 {
    let raw = a.abs_diff(b);
    raw.min(8 - raw)
}

/// Average of two directions that differ by exactly 2 (the diagonal between them).
/// E.g. N(0) and E(2) → NE(1). Handles wraparound (NW(7) and N(0) → still works).
/// Pass 1 uses `segment_midpoint_dir`, the literal native form; this stays as a
/// test-side reference the two are cross-checked against.
#[cfg(test)]
fn midpoint_dir(a: u8, b: u8) -> u8 {
    // The midpoint on the 8-direction wheel. We need the one that's between them,
    // not the one on the opposite side.
    let lo = a.min(b);
    let hi = a.max(b);
    if hi - lo == 2 {
        lo + 1
    } else {
        // Wraparound case: e.g. dir 7 and dir 1 → midpoint is 0
        // or dir 0 and dir 6 → midpoint is 7
        (hi + 1) % 8
    }
}

// ---------------------------------------------------------------------------
// Pass 1: Zigzag smoothing (matches original Path_smooth_corners)
// ---------------------------------------------------------------------------

/// Sentinel for the tube/bridge jump entry in a native direction list.
/// VERA never produces one here — paths containing a non-adjacent step skip
/// smoothing upstream — but the run walker mirrors the native exclusions.
const DIR_TUBE: u8 = 8;

/// Convert a cell list into the native direction list Pass 1 operates on.
/// Returns `None` when any step is not 8-connected (a tube hop).
fn path_to_directions(path: &[(u16, u16)]) -> Option<Vec<u8>> {
    let mut dirs = Vec::with_capacity(path.len().saturating_sub(1));
    for pair in path.windows(2) {
        let d = direction_between(pair[0], pair[1]);
        if d == DIR_INVALID {
            return None;
        }
        dirs.push(d);
    }
    Some(dirs)
}

/// Rebuild a cell list from a start cell and a direction list.
fn directions_to_path(start: (u16, u16), dirs: &[u8]) -> Vec<(u16, u16)> {
    let mut out = Vec::with_capacity(dirs.len() + 1);
    out.push(start);
    let mut pos = (start.0 as i32, start.1 as i32);
    for &d in dirs {
        let (dx, dy) = DIR_DELTAS[(d & 7) as usize];
        pos = (pos.0 + dx, pos.1 + dy);
        out.push((pos.0 as u16, pos.1 as u16));
    }
    out
}

fn step_pos(pos: (i32, i32), dir: u8) -> (i32, i32) {
    let (dx, dy) = DIR_DELTAS[(dir & 7) as usize];
    (pos.0 + dx, pos.1 + dy)
}

/// The cardinal direction lying between two directions 90 degrees apart.
///
/// Native form: `mid = (a + b) >> 1`, sanity-checked against both endpoints and
/// falling back to index 0 for the wrap pair `{1,7}`.
fn segment_midpoint_dir(a: u8, b: u8) -> u8 {
    let mid = (a as u32 + b as u32) >> 1;
    if mid + 1 != b as u32 && mid + 1 != a as u32 {
        0
    } else {
        mid as u8
    }
}

/// Replace one anchor-run / zigzag-run pair, matching `Path_smooth_single_segment`.
///
/// `dirs` starts at the segment base. A run of `run_len` steps of direction `a`
/// followed by `zig_len` steps of `b` (90 degrees apart, both diagonal) becomes
/// `a x (run_len - m)`, `mid x 2m`, `b x (zig_len - m)` with
/// `m = min(run_len, zig_len)` — the step count and the endpoint are preserved,
/// only the interior cells move. Displacement holds because `a + b == 2 * mid`
/// for every legal pair. Every one of the `2m` replacement cells must validate;
/// if any fails, the attempt degrades to `m - 1` pairs starting one step later
/// and retries, down to a single pair, and only then gives up and leaves the
/// zigzag standing.
///
/// Returns the number of leading `dirs` entries the caller's segment base should
/// advance by, and advances `seg_pos` past exactly those entries.
fn smooth_single_segment(
    dirs: &mut [u8],
    run_len: usize,
    zig_len: usize,
    seg_pos: &mut (i32, i32),
    walkable: &dyn Fn(u16, u16) -> bool,
) -> usize {
    let a = dirs[0];
    let b = dirs[run_len];

    // Tube hops are never smoothed; walk the whole pair and consume it.
    if a == DIR_TUBE || b == DIR_TUBE {
        let consumed = run_len + zig_len;
        for &d in dirs.iter().take(consumed) {
            *seg_pos = step_pos(*seg_pos, d);
        }
        return consumed;
    }

    let mid = segment_midpoint_dir(a, b);
    let base = *seg_pos;
    let max_pairs = run_len.min(zig_len);

    for pairs in (1..=max_pairs).rev() {
        let prefix = run_len - pairs;
        let mut repl_start = base;
        for _ in 0..prefix {
            repl_start = step_pos(repl_start, a);
        }
        let mut cursor = repl_start;
        let mut ok = true;
        for _ in 0..(pairs * 2) {
            cursor = step_pos(cursor, mid);
            if cursor.0 < 0 || cursor.1 < 0 || !walkable(cursor.0 as u16, cursor.1 as u16) {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }
        for slot in dirs.iter_mut().skip(prefix).take(pairs * 2) {
            *slot = mid;
        }
        *seg_pos = repl_start;
        return prefix;
    }

    // No replacement size validated: the zigzag stands and the caller resumes at
    // the first entry of the zigzag run.
    let mut pos = base;
    for &d in dirs.iter().take(run_len) {
        pos = step_pos(pos, d);
    }
    *seg_pos = pos;
    run_len
}

/// Pass 1 over a direction list — the run tracker of `Path_smooth_corners`.
///
/// A zigzag is a +/-90 degree turn between the tracked anchor direction and the
/// new one, with tube steps excluded on both sides. The anchor tracker is blanked
/// after any cardinal step, so only diagonal-to-diagonal corners are ever
/// collapsed and the replacement is the cardinal between them. Runs, not pairs,
/// are tracked: the segment helper is called once per run pair with both lengths.
/// The anchor re-seed after a completed smoothing attempt is deliberately NOT
/// parity-filtered — a corner immediately following a smoothed run can anchor on
/// a cardinal.
fn smooth_direction_runs(start: (u16, u16), dirs: &mut [u8], walkable: &dyn Fn(u16, u16) -> bool) {
    let n = dirs.len();
    if n == 0 {
        return;
    }

    let mut anchor = DIR_INVALID;
    let mut run = 0usize;
    let mut base = 0usize;
    let mut zig_dir = DIR_INVALID;
    let mut zig_start = 0usize;
    let mut zig_run = 0usize;
    let mut in_zig = false;
    let mut pos = (start.0 as i32, start.1 as i32);
    let mut base_pos = pos;

    loop {
        // Native top-of-loop guard. The zigzag cursor is the one tested, and it
        // keeps its last value while no zigzag is open.
        if zig_start + zig_run >= n {
            break;
        }
        if in_zig {
            if dirs[zig_start + zig_run] == zig_dir {
                zig_run += 1;
            } else {
                let consumed =
                    smooth_single_segment(&mut dirs[base..], run, zig_run, &mut base_pos, walkable);
                base += consumed;
                run = 1;
                in_zig = false;
                anchor = dirs[base];
                pos = step_pos(base_pos, anchor);
            }
        } else {
            let idx = base + run;
            let d = dirs[idx];
            let diff = d.wrapping_sub(anchor) & 7;
            if d == anchor {
                run += 1;
            } else if (diff == 2 || diff == 6)
                && anchor != DIR_INVALID
                && anchor != DIR_TUBE
                && d != DIR_TUBE
            {
                in_zig = true;
                zig_run = 1;
                zig_start = idx;
                zig_dir = d;
            } else {
                run = 1;
                anchor = if d & 1 == 0 { DIR_INVALID } else { d };
                base_pos = pos;
                base = idx;
            }
            pos = step_pos(pos, d);
        }
        if base + run >= n {
            break;
        }
    }

    // Tail flush: an open zigzag at the end of the array is still smoothed.
    if in_zig {
        smooth_single_segment(&mut dirs[base..], run, zig_run, &mut base_pos, walkable);
    }
}

/// Smooths 90-degree zigzag runs in a path, replacing `2 * min(run, zigzag)`
/// interior steps with the cardinal between the two diagonals.
///
/// Step count and endpoint are preserved; only the interior cells change.
///
/// `walkable(x, y)` must return true if the cell is passable for this unit.
pub fn smooth_path(path: Vec<(u16, u16)>, walkable: &dyn Fn(u16, u16) -> bool) -> Vec<(u16, u16)> {
    // A zigzag needs at least two steps.
    if path.len() < 3 {
        return path;
    }
    let start = path[0];
    let Some(mut dirs) = path_to_directions(&path) else {
        return path;
    };
    smooth_direction_runs(start, &mut dirs, walkable);
    directions_to_path(start, &dirs)
}

/// Smooths a layered path. Layer transitions split Pass 1 — each
/// layer-homogeneous run is smoothed independently, so no replacement crosses a
/// bridge boundary. Pass 1 preserves step count, so the layer vector is carried
/// through unchanged.
pub fn smooth_layered_path(
    path: Vec<(u16, u16)>,
    layers: Vec<MovementLayer>,
    walkable: &dyn Fn(u16, u16, MovementLayer) -> bool,
) -> (Vec<(u16, u16)>, Vec<MovementLayer>) {
    debug_assert_eq!(path.len(), layers.len());
    if path.len() < 3 {
        return (path, layers);
    }

    let mut coords = path;
    let mut seg_start = 0usize;
    while seg_start < coords.len() {
        let layer = layers[seg_start];
        let mut seg_end = seg_start;
        while seg_end + 1 < coords.len() && layers[seg_end + 1] == layer {
            seg_end += 1;
        }
        if seg_end - seg_start >= 2 {
            let segment: Vec<(u16, u16)> = coords[seg_start..=seg_end].to_vec();
            let segment_start = segment[0];
            if let Some(mut dirs) = path_to_directions(&segment) {
                let layer_check = |x: u16, y: u16| walkable(x, y, layer);
                smooth_direction_runs(segment_start, &mut dirs, &layer_check);
                let rebuilt = directions_to_path(segment_start, &dirs);
                debug_assert_eq!(rebuilt.len(), segment.len());
                coords[seg_start..=seg_end].copy_from_slice(&rebuilt);
            }
        }
        seg_start = seg_end + 1;
    }

    (coords, layers)
}

#[cfg(test)]
#[path = "path_smooth_tests.rs"]
mod tests;
