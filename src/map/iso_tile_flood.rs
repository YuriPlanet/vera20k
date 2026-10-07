//! Original MapClass::FloodFillIsoTileType56EB80 control flow.
//!
//! Each cell write, Recalc47D2B0, and radar publication completes before the
//! next neighbor lookup. Membership is the live tile identity; no separate
//! visited set may replace it. The host retains real/dummy allocation identity.

pub(crate) type Coord = (i16, i16);

pub(crate) trait IsoTileFloodHost {
    type Cell: Copy;
    type Error;

    fn lookup(&mut self, requested: Coord) -> Self::Cell;
    fn tile(&self, cell: Self::Cell) -> i32;
    fn write_tile(&mut self, cell: Self::Cell, tile: i32) -> Result<(), Self::Error>;
    fn recalc(&mut self, cell: Self::Cell, level_override: i32) -> Result<(), Self::Error>;
    fn radar(&mut self, cell: Self::Cell);
    /// Native's initial tactical dirty rectangle precedes the equality guard.
    fn initial_screen(&mut self, requested: Coord, cell: Self::Cell);
}

enum Frame {
    Enter {
        requested: Coord,
        recursive_old: Option<i32>,
    },
    Neighbor {
        requested: Coord,
        old: i32,
        direction: u8,
    },
}

/// Explicit depth-first frames preserve native recursion order without tying
/// the connected terrain region's size to the Rust call-stack capacity.
pub(crate) fn replace_connected<H: IsoTileFloodHost>(
    host: &mut H,
    requested: Coord,
    replacement: i32,
    level_override: i32,
) -> Result<(), H::Error> {
    let mut frames = vec![Frame::Enter {
        requested,
        recursive_old: None,
    }];
    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Enter {
                requested,
                recursive_old,
            } => {
                let cell = host.lookup(requested);
                let old = recursive_old.unwrap_or_else(|| {
                    host.initial_screen(requested, cell);
                    host.tile(cell)
                });
                if host.tile(cell) == replacement {
                    continue;
                }
                host.write_tile(cell, replacement)?;
                host.recalc(cell, level_override)?;
                host.radar(cell);
                frames.push(Frame::Neighbor {
                    requested,
                    old,
                    direction: 0,
                });
            }
            Frame::Neighbor {
                requested,
                old,
                direction,
            } => {
                if direction < 7 {
                    frames.push(Frame::Neighbor {
                        requested,
                        old,
                        direction: direction + 1,
                    });
                }
                let delta = crate::util::direction::DIRECTION_DELTAS[usize::from(direction)];
                // 56EB80 steps from its stack request, not retained Cell+24.
                let neighbor = (
                    requested.0.wrapping_add(delta.0 as i16),
                    requested.1.wrapping_add(delta.1 as i16),
                );
                let cell = host.lookup(neighbor);
                if host.tile(cell) == old {
                    // The recursive entry performs its own second lookup.
                    frames.push(Frame::Enter {
                        requested: neighbor,
                        recursive_old: Some(old),
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    // This host matches only the declared oracle callouts. It does not model
    // Recalc semantics or demonstrate the pending production bridge adapter.
    struct ControlHost {
        cells: Vec<(Coord, i32)>,
        dummy: (Coord, i32),
        callback: Option<(Coord, i32)>,
        trace: Vec<Value>,
    }

    impl ControlHost {
        fn cell(&self, index: usize) -> &(Coord, i32) {
            self.cells.get(index).unwrap_or(&self.dummy)
        }

        fn cell_mut(&mut self, index: usize) -> &mut (Coord, i32) {
            self.cells.get_mut(index).unwrap_or(&mut self.dummy)
        }
    }

    impl IsoTileFloodHost for ControlHost {
        type Cell = usize;
        type Error = std::convert::Infallible;

        fn lookup(&mut self, requested: Coord) -> usize {
            let native_slot = i32::from(requested.1) * 512 + i32::from(requested.0);
            if (0..0x40000).contains(&native_slot)
                && let Some(index) = self.cells.iter().position(|(coord, _)| {
                    i32::from(coord.1) * 512 + i32::from(coord.0) == native_slot
                })
            {
                index
            } else {
                self.dummy.0 = requested;
                self.cells.len()
            }
        }

        fn tile(&self, cell: usize) -> i32 {
            self.cell(cell).1
        }

        fn write_tile(&mut self, cell: usize, tile: i32) -> Result<(), Self::Error> {
            self.trace
                .push(json!({"kind":"tile", "coord":self.cell(cell).0, "tile":tile}));
            self.cell_mut(cell).1 = tile;
            Ok(())
        }

        fn recalc(&mut self, cell: usize, level_override: i32) -> Result<(), Self::Error> {
            self.trace
                .push(json!({"kind":"recalc", "coord":self.cell(cell).0, "level":level_override}));
            if let Some((coord, tile)) = self.callback.take() {
                self.cells.iter_mut().find(|row| row.0 == coord).unwrap().1 = tile;
                self.trace
                    .push(json!({"kind":"callback_write", "coord":coord, "tile":tile}));
            }
            Ok(())
        }

        fn radar(&mut self, cell: usize) {
            self.trace
                .push(json!({"kind":"radar", "coord":self.cell(cell).0}));
        }

        fn initial_screen(&mut self, _: Coord, _: usize) {
            self.trace.push(json!({"kind":"screen"}));
        }
    }

    #[test]
    fn connected_tile_replacement_matches_original_control_flow() {
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/iso_tile_flood.json",
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let coord = |row: &Value| {
                (
                    row[0].as_i64().unwrap() as i16,
                    row[1].as_i64().unwrap() as i16,
                )
            };
            let cell = |row: &Value| (coord(row), row[2].as_i64().unwrap() as i32);
            let mut host = ControlHost {
                cells: case["cells"].as_array().unwrap().iter().map(cell).collect(),
                dummy: ((0, 0), -1),
                callback: (!case["first_recalc_write"].is_null())
                    .then(|| cell(&case["first_recalc_write"])),
                trace: Vec::new(),
            };
            replace_connected(
                &mut host,
                coord(&case["start"]),
                case["replacement"].as_i64().unwrap() as i32,
                case["level"].as_i64().unwrap() as i32,
            )
            .unwrap();
            let name = case["name"].as_str().unwrap();
            assert_eq!(json!(host.trace), case["trace"], "{name}: ordered effects");
            let final_cells: Vec<_> = host
                .cells
                .iter()
                .map(|(coord, tile)| json!([coord.0, coord.1, tile]))
                .collect();
            assert_eq!(json!(final_cells), case["final"], "{name}: real cells");
            assert_eq!(
                json!([host.dummy.0.0, host.dummy.0.1, host.dummy.1]),
                case["dummy"],
                "{name}: retained dummy"
            );
        }
    }
}
