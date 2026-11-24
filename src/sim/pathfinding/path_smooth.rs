//! Successful A* path finishing, original42A415/42A41E.
//!
//! The shared native owner below runs42B210/42B420 followed by
//! 42B7F0/42BCA0/42BE20 over retained directions and outgoing-node heights.
//! It preserves packed-word wrapping, concrete Foot entry calls, Map query
//! order, structural bridge lifts, threat thresholds and direction compaction.
//! Reproducible original caller comparisons are in
//! tools/spatial_oracle/astar_path_finishing.{py,json,meta.json}.
//!

const DIR_DELTAS: [(i32, i32); 8] = crate::util::direction::DIRECTION_DELTAS;

pub(crate) trait PathFinishingContext {
    type Cell: Copy;

    fn get_cell(&self, coord: (i16, i16)) -> Self::Cell;
    fn ground_level(&self, cell: Self::Cell) -> i32;
    fn fixed_deck_level(&self, _cell: Self::Cell) -> Option<i32> {
        None
    }
    fn flags(&self, cell: Self::Cell) -> u32;
    /// Foot virtual+1AC(candidate, direction, height, NULL, 1). The entry
    /// owner supplies its class-specific result; finishing accepts only zero.
    fn can_enter(&self, cell: Self::Cell, direction: i32, height: i32) -> Result<u8, String>;
    fn house_threat(&self, coord: (i16, i16)) -> Result<i32, String>;
    fn threat_coefficient(&self) -> Result<crate::util::native_x87::NativeF64Bits, String>;
    /// Cell+116=-1 answers None, which resets the packed coordinate to zero.
    fn tube_exit(&self, cell: Self::Cell) -> Option<(i16, i16)>;
}

/// One outgoing node's retained height (`42AA90` copies it to the node's
/// edge), read in place: from the A*'s own steps, or a corpus's plain array.
pub(crate) trait NodeHeight: Copy {
    fn height(self) -> i32;
}

impl NodeHeight for i32 {
    fn height(self) -> i32 {
        self
    }
}

impl NodeHeight for super::LayeredPathStep {
    fn height(self) -> i32 {
        i32::from(self.path_height())
    }
}

type PackedCell = (i16, i16);

fn packed_step(coord: PackedCell, direction: i32) -> PackedCell {
    let delta = DIR_DELTAS[(direction & 7) as usize];
    (
        coord.0.wrapping_add(delta.0 as i16),
        coord.1.wrapping_add(delta.1 as i16),
    )
}

fn packed_sub(lhs: PackedCell, rhs: PackedCell) -> PackedCell {
    (lhs.0.wrapping_sub(rhs.0), lhs.1.wrapping_sub(rhs.1))
}

fn packed_distance(delta: PackedCell) -> i32 {
    i32::from(delta.0).abs().max(i32::from(delta.1).abs())
}

fn follow_direction<C: PathFinishingContext>(
    coord: PackedCell,
    direction: i32,
    ctx: &C,
) -> PackedCell {
    if direction == 8 {
        ctx.tube_exit(ctx.get_cell(coord)).unwrap_or((0, 0))
    } else {
        packed_step(coord, direction)
    }
}

fn next_finishing_height<C: PathFinishingContext>(previous: i32, cell: C::Cell, ctx: &C) -> i32 {
    use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS;

    let ground = ctx.ground_level(cell);
    // The 42B420 and 42BE20 bridge-height checks keep the incoming height
    // only when it matches this cell's deck. Keep the native read order for
    // synthetic cells and for the mutable map Dummy.
    if let Some(deck) = ctx.fixed_deck_level(cell) {
        return if previous == deck { deck } else { ground };
    }
    if previous.wrapping_sub(ground) == BRIDGE_DECK_HEIGHT_LEVELS && ctx.flags(cell) & 0x100 != 0 {
        ground.wrapping_add(BRIDGE_DECK_HEIGHT_LEVELS)
    } else {
        ground
    }
}

fn threat_at_least(
    threat: i32,
    coefficient: crate::util::native_x87::X87Value,
    threshold: crate::util::native_x87::NativeF64Bits,
) -> bool {
    use crate::util::native_x87::{X87Chop53, X87Ordering};
    let product = X87Chop53::mul(X87Chop53::load_i32(threat), coefficient);
    let limit =
        X87Chop53::load_f64(threshold).expect("callers pass the native 1.0 and 0.01 thresholds");
    X87Chop53::compare(product, limit) != X87Ordering::Less
}

/// Original42B420. Replacement validation keeps the original outgoing-node
/// height array, reads the retained Cell after entry, and performs one extra
/// lookahead lookup even after a rejection. Native comparisons are in
/// tools/spatial_oracle/astar_path_finishing.{py,json,meta.json}.
fn smooth_native_segment<C: PathFinishingContext, H: NodeHeight>(
    dirs: &mut [i32],
    heights: &[H],
    run_len: usize,
    zig_len: usize,
    base: &mut PackedCell,
    ctx: &C,
) -> Result<usize, String> {
    let first = dirs[0];
    let second = dirs[run_len];
    if first == 8 || second == 8 {
        let consumed = run_len + zig_len;
        for &direction in &dirs[..consumed] {
            *base = packed_step(*base, direction);
        }
        return Ok(consumed);
    }
    let mut midpoint = (first + second) >> 1;
    if midpoint + 1 != first && midpoint + 1 != second {
        midpoint = 0;
    }
    let mut origin = *base;
    let pairs = run_len.min(zig_len);
    for &direction in &dirs[..run_len - pairs] {
        origin = follow_direction(origin, direction, ctx);
    }
    let coefficient = crate::util::native_x87::X87Chop53::load_f64(ctx.threat_coefficient()?)
        .map_err(|e| e.to_string())?;
    for pairs in (1..=pairs).rev() {
        let prefix = run_len - pairs;
        let mut height = heights[prefix].height();
        let mut cursor = packed_step(origin, midpoint);
        let mut cell = ctx.get_cell(cursor);
        let mut rejected = false;
        for _ in 0..pairs * 2 {
            rejected = ctx.can_enter(cell, midpoint, height)? != 0
                || ctx.flags(cell) & 0x40000 != 0
                || threat_at_least(
                    ctx.house_threat(origin)?,
                    coefficient,
                    crate::util::native_x87::NativeF64Bits::ONE,
                );
            cursor = packed_step(cursor, midpoint);
            cell = ctx.get_cell(cursor);
            height = next_finishing_height(height, cell, ctx);
            if rejected {
                break;
            }
        }
        if !rejected {
            dirs[prefix..prefix + pairs * 2].fill(midpoint);
            for &direction in &dirs[..prefix] {
                *base = follow_direction(*base, direction, ctx);
            }
            return Ok(prefix);
        }
        origin = packed_step(origin, first);
    }
    for &direction in &dirs[..run_len] {
        *base = packed_step(*base, direction);
    }
    Ok(run_len)
}

/// Original42B210, reached at42A415. Directions exclude the -1 terminator;
/// heights are the unchanged outgoing-node array from42AA90. No layer split
/// occurs in the original pass.
pub(crate) fn smooth_corners<C: PathFinishingContext, H: NodeHeight>(
    start: PackedCell,
    dirs: &mut [i32],
    heights: &[H],
    ctx: &C,
) -> Result<(), String> {
    debug_assert_eq!(dirs.len(), heights.len());
    let n = dirs.len();
    let (mut anchor, mut run, mut base) = (-1i32, 0usize, 0usize);
    let (mut zig_dir, mut zig_start, mut zig_run) = (-1i32, 0usize, 0usize);
    let mut in_zig = false;
    let mut pos = start;
    let mut base_pos = start;
    while zig_start + zig_run < n {
        if in_zig {
            if dirs[zig_start + zig_run] == zig_dir {
                zig_run += 1;
            } else {
                base += smooth_native_segment(
                    &mut dirs[base..],
                    &heights[base..],
                    run,
                    zig_run,
                    &mut base_pos,
                    ctx,
                )?;
                run = 1;
                in_zig = false;
                anchor = dirs[base];
                pos = packed_step(base_pos, anchor);
            }
        } else {
            let index = base + run;
            let direction = dirs[index];
            let diff = direction.wrapping_sub(anchor) & 7;
            if direction == anchor {
                run += 1;
            } else if (diff == 2 || diff == 6) && anchor != -1 && anchor != 8 && direction != 8 {
                in_zig = true;
                zig_run = 1;
                zig_start = index;
                zig_dir = direction;
            } else {
                run = 1;
                anchor = if direction & 1 == 0 { -1 } else { direction };
                base_pos = pos;
                base = index;
            }
            pos = follow_direction(pos, direction, ctx);
        }
        if base + run >= n {
            break;
        }
    }
    if in_zig {
        smooth_native_segment(
            &mut dirs[base..],
            &heights[base..],
            run,
            zig_run,
            &mut base_pos,
            ctx,
        )?;
    }
    Ok(())
}

/// Original42BCA0: walk backwards through the retained direction array,
/// ignoring delete markers until displacement starts growing after a plateau.
fn find_split_anchor(
    dirs: &[i32],
    end: usize,
    lower: usize,
    mut coord: PackedCell,
) -> (usize, PackedCell) {
    let mut delta = (0i16, 0i16);
    let mut high_water = 0;
    let mut plateau = false;
    for index in (lower..=end).rev() {
        if dirs[index] == -2 {
            continue;
        }
        let backward = dirs[index].wrapping_sub(4) & 7;
        delta = packed_step(delta, backward);
        coord = packed_step(coord, backward);
        let distance = packed_distance(delta);
        if distance > high_water {
            high_water = distance;
            if plateau {
                return (index + 1, packed_step(coord, backward.wrapping_sub(4) & 7));
            }
        } else {
            plateau = true;
        }
    }
    (lower, coord)
}

/// Original42BE20. Try diagonal-first, then cardinal-first; failure does not
/// mutate the direction array. Tail flush allows fewer than four threatening
/// cells; the interior attempt requires zero. Heights remain the retained
/// originals even when this function writes delete markers.
fn reroute_straight<C: PathFinishingContext>(
    dirs: &mut [i32],
    start: PackedCell,
    delta: PackedCell,
    initial_height: i32,
    tail: bool,
    ctx: &C,
) -> Result<(), String> {
    use crate::util::native_x87::{NativeF64Bits, X87Chop53, X87Ordering};
    let (dx, dy) = (i32::from(delta.0), i32::from(delta.1));
    let diagonal = if dx < 0 {
        if dy < 0 { 7 } else { 5 }
    } else if dy < 0 {
        1
    } else {
        3
    };
    //42BE6B..42BE8F compares dx-dy, then SETLE(dx+dy). The two
    //nonpositive branches choose west for dx<=dy and north otherwise.
    let cardinal = if dx <= dy {
        if dx + dy <= 0 { 6 } else { 4 }
    } else if dx + dy <= 0 {
        0
    } else {
        2
    };
    let minor = dx.abs().min(dy.abs()) as usize;
    let major = dx.abs().max(dy.abs()) as usize - minor;
    let coefficient = X87Chop53::load_f64(ctx.threat_coefficient()?).map_err(|e| e.to_string())?;
    let threshold = X87Chop53::load_f64(NativeF64Bits::from_bits(0x3ee4_f8b5_88e3_68f1))
        .expect("native1e-5 threshold");
    let query_threat = X87Chop53::compare(coefficient, threshold) == X87Ordering::Greater;
    for ((first_dir, first_count), (second_dir, second_count)) in [
        ((diagonal, minor), (cardinal, major)),
        ((cardinal, major), (diagonal, minor)),
    ] {
        //42BEF9 skips this orientation when its first run is empty.
        if first_count == 0 {
            continue;
        }
        let mut coord = start;
        let mut height = initial_height;
        let mut threatening = 0;
        let mut rejected = false;
        for (direction, count) in [(first_dir, first_count), (second_dir, second_count)] {
            for _ in 0..count {
                if rejected {
                    break;
                }
                coord = packed_step(coord, direction);
                let cell = ctx.get_cell(coord);
                if query_threat
                    && threat_at_least(
                        ctx.house_threat(coord)?,
                        coefficient,
                        NativeF64Bits::from_bits(0x3f84_7ae1_47ae_147b),
                    )
                {
                    threatening += 1;
                }
                rejected = ctx.can_enter(cell, direction, height)? != 0
                    || ctx.flags(cell) & 0x40000 != 0
                    || threatening >= 4
                    || (!tail && threatening >= 1);
                height = next_finishing_height(height, cell, ctx);
            }
        }
        if !rejected {
            dirs[..first_count].fill(first_dir);
            dirs[first_count..first_count + second_count].fill(second_dir);
            dirs[first_count + second_count..].fill(-2);
            return Ok(());
        }
    }
    Ok(())
}

/// Original42B7F0, reached at42A41E. Its scan visits at most20 input slots;
/// compaction subsequently retains every surviving slot, including the suffix
/// outside that window. Only directions/count change, never height storage.
pub(crate) fn optimize_straight_segments<C: PathFinishingContext, H: NodeHeight>(
    start: PackedCell,
    dirs: &mut Vec<i32>,
    heights: &[H],
    ctx: &C,
) -> Result<(), String> {
    debug_assert_eq!(dirs.len(), heights.len());
    let mut pos = start;
    let mut index = 0;
    let mut anchor = (0i16, 0i16);
    let mut anchor_index = 0;
    let mut split_lower = 0;
    let mut segment_delta = (0i16, 0i16);
    let mut total_delta = (0i16, 0i16);
    let (mut max_x, mut max_y, mut high_water) = (0, 0, 0);
    while index < dirs.len() && index < 20 {
        let direction = dirs[index];
        if direction == 8 {
            //42B88A..42B890 uses direction-table entry0 here, unlike42B210's
            //Tube lookup. It then starts a new optimization window.
            pos = packed_step(pos, 0);
            index += 1;
            anchor = (0, 0);
            anchor_index = index;
            split_lower = index;
            segment_delta = (0, 0);
            total_delta = (0, 0);
            max_x = 0;
            max_y = 0;
            high_water = 0;
            continue;
        }
        if direction == -2 {
            index += 1;
            continue;
        }
        let next_segment = packed_step(segment_delta, direction);
        let next_total = packed_step(total_delta, direction);
        let abs_x = i32::from(next_segment.0).abs();
        let abs_y = i32::from(next_segment.1).abs();
        if abs_x < max_x || abs_y < max_y {
            if anchor != (0, 0) {
                split_lower = anchor_index;
                total_delta = packed_sub(anchor, pos);
                high_water = packed_distance(total_delta);
            }
            anchor = pos;
            anchor_index = index;
            segment_delta = (0, 0);
            max_x = 0;
            max_y = 0;
            //The reversal retries this same direction from the new origin.
            continue;
        }
        segment_delta = next_segment;
        pos = packed_step(pos, direction);
        let mut distance = packed_distance(next_total);
        if distance <= high_water {
            let (split, split_coord) = find_split_anchor(dirs, index, split_lower, pos);
            reroute_straight(
                &mut dirs[split..=index],
                split_coord,
                packed_sub(pos, split_coord),
                heights[split].height(),
                false,
                ctx,
            )?;
            distance = high_water;
        }
        high_water = distance;
        total_delta = next_total;
        max_x = abs_x;
        max_y = abs_y;
        index += 1;
    }
    if anchor != (0, 0)
        && packed_distance(packed_sub(pos, anchor)) < index as i32 - anchor_index as i32 - 1
    {
        let (split, split_coord) = find_split_anchor(dirs, index - 1, anchor_index, pos);
        reroute_straight(
            &mut dirs[split..index],
            split_coord,
            packed_sub(pos, split_coord),
            heights[split].height(),
            true,
            ctx,
        )?;
    }
    dirs.retain(|&direction| direction != -2);
    Ok(())
}

/// The original successful-search sequence42A415/42A41E. Returned directions
/// exclude the native terminator; the retained outgoing-node heights are input
/// storage and deliberately remain unchanged after compaction.
pub(crate) fn finish_path<C: PathFinishingContext, H: NodeHeight>(
    start: PackedCell,
    dirs: &mut Vec<i32>,
    heights: &[H],
    ctx: &C,
) -> Result<(), String> {
    smooth_corners(start, dirs, heights, ctx)?;
    optimize_straight_segments(start, dirs, heights, ctx)
}

#[cfg(all(test, any(tests_sim_pathfinding, tests_sim_movement)))]
#[path = "path_finishing_native_tests.rs"]
mod native_tests;
