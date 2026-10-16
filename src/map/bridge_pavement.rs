//! Native ToggleBridgePavement56E990: raw bit13, independent of bridge roles.
//! Evidence: tools/spatial_oracle/bridge_pavement.py/json. Each child captures
//! its own current tile; the parent retains its membership tile across callbacks.

pub const DAMAGED_PAVEMENT: u32 = 0x2000;
pub(crate) type Coord = (i16, i16);

pub(crate) trait PavementHost {
    type Cell: Copy;
    fn lookup(&mut self, requested: Coord) -> Self::Cell;
    fn tile(&self, cell: Self::Cell) -> i32;
    fn flags(&self, cell: Self::Cell) -> u32;
    fn write_flags(&mut self, cell: Self::Cell, flags: u32);
    /// Original5471F0 queries the pristine TMP and normalizes unsigned11A
    /// modulo width*height. A sparse entry is false, not an invalid-subtile error.
    fn has_damaged_data(&mut self, cell: Self::Cell) -> bool;
    fn initial_screen(&mut self, requested: Coord, cell: Self::Cell);
    fn radar(&mut self, cell: Self::Cell);
}

enum Frame {
    Enter {
        requested: Coord,
        kickoff: bool,
    },
    Neighbor {
        requested: Coord,
        tile: i32,
        direction: u8,
    },
}

/// All established native callers pass0/1. Preserve depth-first order without
/// tying a large connected pavement footprint to Rust's call-stack limit.
pub(crate) fn set_connected<H: PavementHost>(host: &mut H, requested: Coord, state: bool) {
    let mut frames = vec![Frame::Enter {
        requested,
        kickoff: true,
    }];
    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Enter { requested, kickoff } => {
                let cell = host.lookup(requested);
                if kickoff {
                    if matches!(host.tile(cell), 0xffff | 0xff) || !host.has_damaged_data(cell) {
                        continue;
                    }
                    host.initial_screen(requested, cell);
                }
                let flags = host.flags(cell);
                if (flags & DAMAGED_PAVEMENT != 0) == state {
                    continue;
                }
                let tile = host.tile(cell);
                host.write_flags(cell, (flags & !DAMAGED_PAVEMENT) | (u32::from(state) << 13));
                host.radar(cell);
                frames.push(Frame::Neighbor {
                    requested,
                    tile,
                    direction: 0,
                });
            }
            Frame::Neighbor {
                requested,
                tile,
                direction,
            } => {
                if direction < 7 {
                    frames.push(Frame::Neighbor {
                        requested,
                        tile,
                        direction: direction + 1,
                    });
                }
                let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[usize::from(direction)];
                let neighbor = (
                    requested.0.wrapping_add(dx as i16),
                    requested.1.wrapping_add(dy as i16),
                );
                let cell = host.lookup(neighbor);
                if host.tile(cell) == tile {
                    frames.push(Frame::Enter {
                        requested: neighbor,
                        kickoff: false,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "bridge_pavement_tests.rs"]
mod tests;
