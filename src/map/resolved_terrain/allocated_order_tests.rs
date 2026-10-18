use super::*;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::{test_flat_cell, test_grid};

/// Every cursor state and cell of a walk from reset, and the state it ends
/// in.
fn walk(
    terrain: &ResolvedTerrainGrid,
    size_width: i32,
) -> (
    Vec<(NativeAllocatedCellCursor, (u16, u16))>,
    NativeAllocatedCellCursor,
) {
    let mut cursor = NativeAllocatedCellCursor::reset(size_width);
    let mut steps = Vec::new();
    while let Some(identity) = cursor.next_in_terrain(terrain, size_width) {
        let NativeCellIdentity::Real(index) = identity else {
            unreachable!("a walk yields real cells")
        };
        let cell = &terrain.cells()[index];
        steps.push((cursor, (cell.rx, cell.ry)));
    }
    (steps, cursor)
}

fn assert_matches_walk(terrain: &ResolvedTerrainGrid, size_width: i32) {
    let order = AllocatedCellOrder::build(terrain, size_width).expect("regular terrain");
    let (steps, end) = walk(terrain, size_width);
    assert_eq!(order.len() as usize, steps.len(), "width {size_width}");
    assert_eq!(order.end(), end, "width {size_width}");
    let mut missing = Vec::new();
    for (rank, &(after, cell)) in steps.iter().enumerate() {
        let rank = rank as u32;
        assert_eq!(order.cell(rank), cell, "width {size_width} rank {rank}");
        assert_eq!(
            order.cursor_after(rank),
            after,
            "width {size_width} rank {rank}"
        );
        assert_eq!(
            order.rank((i32::from(cell.0), i32::from(cell.1))),
            Some(rank)
        );
        let mask = ROW_NEIGHBORS
            .iter()
            .enumerate()
            .filter(|&(_, &(dx, dy))| {
                terrain
                    .native_fixed_cell_index(
                        (cell.0 as i16).wrapping_add(dx),
                        (cell.1 as i16).wrapping_add(dy),
                    )
                    .is_none()
            })
            .fold(0u8, |mask, (bit, _)| mask | 1 << bit);
        if mask != 0 {
            missing.push((rank, mask));
        }
    }
    assert_eq!(order.missing_neighbors_through(u32::MAX), missing);
    let walked: std::collections::BTreeSet<_> = steps.iter().map(|&(_, cell)| cell).collect();
    for y in -2..i32::from(terrain.height()) + 2 {
        for x in -2..i32::from(terrain.width()) + 2 {
            let on_walk = u16::try_from(x)
                .ok()
                .zip(u16::try_from(y).ok())
                .is_some_and(|cell| walked.contains(&cell));
            assert_eq!(
                order.rank((x, y)).is_some(),
                on_walk,
                "width {size_width} ({x},{y})"
            );
        }
    }
}

#[test]
fn ranks_cells_and_cursors_follow_the_native_walk() {
    // Rectangular storage ends the walk at the first cell past the Size
    // diamond, or inside it when the storage is smaller.
    for (size_width, size_height) in [(2, 2), (3, 2), (6, 4), (5, 7), (8, 8), (13, 6)] {
        let side = (size_width + size_height) as u16;
        let terrain = test_grid(side, side, test_flat_cell);
        assert_matches_walk(&terrain, size_width);
        let narrow = test_grid(side, side / 2, test_flat_cell);
        assert_matches_walk(&narrow, size_width);
    }
}

#[test]
fn an_unallocated_cell_ends_the_walk_and_counts_as_missing() {
    let terrain = test_grid(12, 12, test_flat_cell);
    let (steps, _) = walk(&terrain, 6);
    let mut allocated: Vec<_> = steps.iter().map(|&(_, cell)| cell).collect();
    // A hole in the middle ends the walk there; the cells past it stay
    // allocated neighbours of the walk.
    allocated.remove(30);
    let mut holed = terrain.clone();
    holed.test_set_native_allocated_cells(&allocated);
    assert_matches_walk(&holed, 6);
    assert_eq!(AllocatedCellOrder::build(&holed, 6).unwrap().len(), 30);
}

#[test]
fn irregular_terrain_has_no_order() {
    let terrain = test_grid(10, 10, test_flat_cell);
    assert!(AllocatedCellOrder::build(&terrain, 1).is_none());
    let mut shuffled = terrain.cells().to_vec();
    shuffled.swap(11, 12);
    assert!(
        AllocatedCellOrder::build(&ResolvedTerrainGrid::from_cells(10, 10, shuffled), 4).is_none()
    );
    let wide = test_grid(512, 8, test_flat_cell);
    assert!(AllocatedCellOrder::build(&wide, 4).is_none());
}

#[test]
fn the_cache_keeps_the_first_width() {
    let terrain = test_grid(10, 10, test_flat_cell);
    let first = terrain.allocated_cell_order(4).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &terrain.allocated_cell_order(4).unwrap()
    ));
    let other = terrain.allocated_cell_order(5).unwrap();
    assert_eq!(other.len(), walk(&terrain, 5).0.len() as u32);
}

#[test]
fn interior_cells_are_those_whose_neighbourhood_looks_up_itself() {
    let terrain = test_grid(12, 10, test_flat_cell);
    let (steps, _) = walk(&terrain, 6);
    let mut allocated: Vec<_> = steps.iter().map(|&(_, cell)| cell).collect();
    allocated.remove(30);
    let mut holed = terrain.clone();
    holed.test_set_native_allocated_cells(&allocated);
    for terrain in [&terrain, &holed] {
        let interior = terrain.interior_cells().expect("regular terrain");
        let query = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
        for y in 0..terrain.height() + 2 {
            for x in 0..terrain.width() + 2 {
                let looks_up_itself = (-1..=1).all(|dy| {
                    (-1..=1).all(|dx| {
                        let at = (x as i16 + dx, y as i16 + dy);
                        match query.lookup(at) {
                            NativeCellIdentity::Real(index) => {
                                let cell = &terrain.cells()[index];
                                (cell.rx as i16, cell.ry as i16) == at
                                    && index
                                        == at.1 as usize * usize::from(terrain.width())
                                            + at.0 as usize
                            }
                            NativeCellIdentity::Dummy => false,
                        }
                    })
                });
                assert_eq!(interior.contains((x, y)), looks_up_itself, "({x},{y})");
            }
        }
    }
    let wide = test_grid(512, 8, test_flat_cell);
    assert!(wide.interior_cells().is_none());
}
