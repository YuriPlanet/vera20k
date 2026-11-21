//! Cell properties (appendix 7.6), the global rules reference (7.8) and
//! `find` over cells (scope 2), read through their owners.
//!
//! Residuals:
//! - `CellTag` reads 0 for a cell without a tag; a tagged cell answers
//!   `Unsupported` until tags have references.
//! - `Passable.<SpeedType>`, `Revealed` and `Placeable` are not read yet.
//! - Global `Difficulty`, `GameMode`, `MissionTimer`, `MissionTimerRunning`,
//!   `AmbientLight` and `IonStormActive` answer `Unsupported`.

use vera20k_wasm_ext::{Area, FindPage, Invalid, NAME_OVERLAY, Query, Status};

use super::{Host, Ref, cell_ref, decode_ref, type_ref};

impl Host<'_, '_> {
    /// A cell's property (appendix 7.6). A cell outside the map is
    /// `NotFound`.
    pub(super) fn cell_property(&self, x: u16, y: u16, name: &str) -> (i64, Status) {
        let Some(terrain) = self.sim.resolved_terrain() else {
            return (0, Status::Unsupported);
        };
        let Some(cell) = terrain.cell(x, y) else {
            return (0, Status::NotFound);
        };
        let overlay = || self.sim.overlay_grid().map(|grid| grid.cell(x, y));
        let value = match name {
            "LandType" => i64::from(cell.land_type),
            "Height" => i64::from(cell.level),
            "Overlay" => match overlay().and_then(|o| o.overlay_id) {
                Some(id) => type_ref(NAME_OVERLAY, u32::from(id)),
                None => 0,
            },
            // CellClass::Get_Tiberium_Value @ 0x00485020.
            "Ore" => {
                let Some(cell) = overlay() else {
                    return (0, Status::Unsupported);
                };
                crate::map::tiberium_cell::tiberium_value(
                    cell.overlay_id,
                    cell.overlay_data,
                    self.rules.overlay_registry(),
                    &self.rules.tiberium_types,
                )
                .map_or(0, i64::from)
            }
            "Waypoint" => (0..crate::map::waypoints::WAYPOINT_SLOT_COUNT as i32)
                .find(|&n| {
                    self.sim.session.waypoint_is_valid(n)
                        && self.sim.session.waypoint_cell(n) == Some((x as i16, y as i16))
                })
                .map_or(-1, i64::from),
            "CellTag" => {
                let identity = terrain.native_cell_identity((x as i16, y as i16));
                if self.sim.trigger_runtime.cell_tag(identity).is_some() {
                    return (0, Status::Unsupported);
                }
                0
            }
            _ => return (0, Status::Unsupported),
        };
        (value, Status::Ok)
    }

    /// A global rules reference property (appendix 7.8).
    pub(super) fn global_property(&self, name: &str) -> (i64, Status) {
        let value = match name {
            "Frame" => i64::from(self.sim.session.binary_frame()),
            "PlayerCount" => i64::from(crate::sim::crates::human_player_count(self.sim)),
            "GameSpeed" => i64::from(self.sim.session.game_options().game_speed),
            _ => return (0, Status::Unsupported),
        };
        (value, Status::Ok)
    }

    /// `find` over cells, in cell index order (y, then x). A query with a
    /// type or owner answers `Unsupported`.
    pub(super) fn find_cells(
        &self,
        query: &Query,
        cap: usize,
    ) -> Result<(FindPage, Status), Invalid> {
        if query.type_ref != 0 || query.owner != 0 {
            return Ok((FindPage::default(), Status::Unsupported));
        }
        let Some(terrain) = self.sim.resolved_terrain() else {
            return Ok((FindPage::default(), Status::Unsupported));
        };
        let (width, height) = (i64::from(terrain.width()), i64::from(terrain.height()));
        // The cells to scan: the map, or the area's bounding box.
        let (centre, x0, y0, x1, y1) = match query.area {
            Area::Map => (None, 0, 0, width, height),
            Area::Circle { cell, radius } => {
                let (cx, cy) = area_cell(cell)?;
                let cells = i64::from(radius) / 256 + 1;
                (
                    Some((cx, cy)),
                    cx - cells,
                    cy - cells,
                    cx + cells + 1,
                    cy + cells + 1,
                )
            }
            Area::Rect {
                cell,
                width: w,
                height: h,
            } => {
                let (cx, cy) = area_cell(cell)?;
                (None, cx, cy, cx + i64::from(w), cy + i64::from(h))
            }
        };
        let (x0, y0, x1, y1) = (x0.max(0), y0.max(0), x1.min(width), y1.min(height));
        let mut page = FindPage::default();
        'rows: for y in y0..y1 {
            for x in x0..x1 {
                let key = (y << 16) | x;
                if key < query.after {
                    continue;
                }
                if page.found.len() == cap {
                    break 'rows;
                }
                let (Ok(rx), Ok(ry)) = (u16::try_from(x), u16::try_from(y)) else {
                    continue;
                };
                if terrain.cell(rx, ry).is_none() {
                    continue;
                }
                page.scanned += 1;
                if let (Some((cx, cy)), Area::Circle { radius, .. }) = (centre, query.area) {
                    let (dx, dy) = ((x - cx) * 256, (y - cy) * 256);
                    if dx * dx + dy * dy > i64::from(radius) * i64::from(radius) {
                        continue;
                    }
                }
                let reference = cell_ref(rx, ry);
                match self.passes(reference, query)? {
                    Some(true) => page.found.push((key, reference)),
                    Some(false) => {}
                    None => return Ok((FindPage::default(), Status::Unsupported)),
                }
            }
        }
        Ok((page, Status::Ok))
    }
}

/// An area's cell, which must be a cell reference.
fn area_cell(cell: i64) -> Result<(i64, i64), Invalid> {
    match decode_ref(cell)? {
        Some(Ref::Cell { x, y }) => Ok((i64::from(x), i64::from(y))),
        _ => Err(Invalid(format!("{cell:#x} is not a cell reference"))),
    }
}
