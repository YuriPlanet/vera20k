//! Cell properties (appendix 7.6), the global rules reference (7.8) and
//! `find` over cells (scope 2), read through their owners.
//!
//! | Property | Owner |
//! | --- | --- |
//! | `Passable.<SpeedType>` | the land row's cost for that SpeedType (`ResolvedTerrainGrid::land_speed_costs`, the 89EA40 table CheckCellPassability reads): passable when not zero. Terrain only; objects and overlays standing there are not asked |
//! | `Revealed` | the viewpoint house's revealed map (`FogState::is_cell_revealed`) |
//! | `Placeable` | `production::is_within_build_area` (Display4A8EB0's proximity) and `build_site::can_place_building_at` (BuildingTypeClass::CanPlaceAt `0x00464AC0`) for the viewpoint house and the query's TechnoType at this cell as the foundation's top left; 0 for a type that is not a building. Only in `find` with a type, else `Unsupported` |
//! | `Difficulty` | the trigger difficulty (`Simulation::trigger_difficulty`), the index the `TriggerDef.difficulty` mask uses |
//! | `MissionTimer`, `MissionTimerRunning` | the scenario countdown (TAction 23..27): frames left, also while stopped, and whether it runs |
//! | `AmbientLight` | the scenario lighting's current ambient level |
//! | `IonStormActive` | `lightning_storm::raging` (LightningStorm::IsActive `0x0053A100`) |
//! | `Zone.<MovementZone>` | `ZoneGrid::get_zone_id_native` (MapClass::GetZoneID `0x0056D230`, bridge check off): the connected area the pathfinder's zone precheck compares; `Unsupported` before the zones are built |
//! | `Obstructed` | a building in the cell's ground list (`OccupancyGrid::first_building_on_layer`, Look_up_building_in_cell `0x0047C520`), or an overlay RecalcZoneType (`0x00483C80`) makes Wall or Impassable (`overlay_reduced_zone_type`: walls, rocks, wheel speed 0) |
//! | `AutomationAllowed` | the room switch "Allow automation" (`ExtModules::automation_allowed`, from `PersonalRules::automation_allowed`), 0 by default |
//!
//! Residuals:
//! - `CellTag` reads 0 for a cell without a tag; a tagged cell answers
//!   `Unsupported` until tags have references.
//! - Global `GameMode` (a string) answers `Unsupported`: string IDs are not
//!   kept yet.
//! - A cell `find` with an owner answers `Unsupported`; its type serves only
//!   `Placeable`.
//! - `Zone.<MovementZone>` reads the cell's own (ground) zone: a bridge deck
//!   cell answers the zone below it, not the deck's.
//! - `Obstructed` leaves out trees and other terrain objects, units and
//!   impassable land (`Passable.<SpeedType>` reads the land).

use vera20k_wasm_ext::{Area, FindPage, Invalid, NAME_OVERLAY, NAME_TECHNO_TYPE, Query, Status};

use super::{Reader, Ref, View, cell_ref, decode_ref, techno_type, type_ref};
use crate::map::resolved_terrain::{overlay_reduced_zone_type, zone_class};
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::ObjectCategory;
use crate::sim::movement::locomotor::MovementLayer;

/// The MovementZone a `Zone.<MovementZone>` property names.
fn zone_movement_zone(name: &str) -> Option<MovementZone> {
    Some(match name.strip_prefix("Zone.")? {
        "Normal" => MovementZone::Normal,
        "Crusher" => MovementZone::Crusher,
        "Destroyer" => MovementZone::Destroyer,
        "AmphibiousDestroyer" => MovementZone::AmphibiousDestroyer,
        "AmphibiousCrusher" => MovementZone::AmphibiousCrusher,
        "Amphibious" => MovementZone::Amphibious,
        "Subterrannean" => MovementZone::Subterranean,
        "Infantry" => MovementZone::Infantry,
        "InfantryDestroyer" => MovementZone::InfantryDestroyer,
        "Fly" => MovementZone::Fly,
        "Water" => MovementZone::Water,
        "WaterBeach" => MovementZone::WaterBeach,
        "CrusherAll" => MovementZone::CrusherAll,
        _ => return None,
    })
}

/// The SpeedType a `Passable.<SpeedType>` property names.
fn passable_speed_type(name: &str) -> Option<SpeedType> {
    Some(match name.strip_prefix("Passable.")? {
        "Foot" => SpeedType::Foot,
        "Track" => SpeedType::Track,
        "Wheel" => SpeedType::Wheel,
        "Hover" => SpeedType::Hover,
        "Winged" => SpeedType::Winged,
        "Float" => SpeedType::Float,
        "Amphibious" => SpeedType::Amphibious,
        "FloatBeach" => SpeedType::FloatBeach,
        _ => return None,
    })
}

impl Reader<'_> {
    /// A cell's property (appendix 7.6), viewpoint properties for `view`.
    /// A cell outside the map is `NotFound`.
    pub(super) fn cell_property(&self, x: u16, y: u16, name: &str, view: View) -> (i64, Status) {
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
            "Revealed" => match self.viewpoint(view) {
                Ok(house) => i64::from(self.sim.fog.is_cell_revealed(house, x, y)),
                Err(status) => return (0, status),
            },
            "Placeable" => return self.placeable(x, y, view),
            "Obstructed" => {
                let building = self
                    .sim
                    .occupancy()
                    .first_building_on_layer(x, y, MovementLayer::Ground)
                    .is_some();
                let blocking_overlay = overlay()
                    .and_then(|cell| cell.overlay_id)
                    .and_then(|id| self.rules.overlay_registry().flags(id))
                    .and_then(|flags| overlay_reduced_zone_type(Some(flags)))
                    .is_some_and(|zone| matches!(zone, zone_class::WALL | zone_class::IMPASSABLE));
                i64::from(building || blocking_overlay)
            }
            _ if name.starts_with("Zone.") => {
                let (Some(movement_zone), Some(zones)) =
                    (zone_movement_zone(name), self.sim.zone_grid.as_ref())
                else {
                    return (0, Status::Unsupported);
                };
                match zones.get_zone_id_native(terrain, (x, y), movement_zone, false) {
                    Some(zone) => i64::from(zone),
                    None => return (0, Status::Unsupported),
                }
            }
            _ => match passable_speed_type(name) {
                Some(speed) => i64::from(
                    terrain
                        .land_speed_costs(i32::from(cell.land_type))
                        .cost_for_speed_type(speed)
                        .is_some_and(|cost| !cost.is_zero()),
                ),
                None => return (0, Status::Unsupported),
            },
        };
        (value, Status::Ok)
    }

    /// `Placeable`: whether the viewpoint house may place the query's type
    /// with its foundation's top left at (`x`, `y`) (module docs).
    fn placeable(&self, x: u16, y: u16, view: View) -> (i64, Status) {
        let Some(id) = view.placing else {
            return (0, Status::Unsupported);
        };
        let house = match self.viewpoint(view) {
            Ok(house) => house,
            Err(status) => return (0, status),
        };
        let Some(object) = i32::try_from(id)
            .ok()
            .and_then(|id| techno_type(self.rules, id))
        else {
            return (0, Status::NotFound);
        };
        if object.category != ObjectCategory::Building {
            return (0, Status::Ok);
        }
        let Some(owner) = self.sim.interner.try_resolve(house) else {
            return (0, Status::Gone);
        };
        let placeable =
            crate::sim::production::is_within_build_area(self.sim, self.rules, owner, object, x, y)
                && crate::sim::build_site::can_place_building_at(
                    self.sim,
                    self.rules,
                    self.overlay_registry,
                    object,
                    (x as i16, y as i16),
                    Some(house),
                    None,
                    None,
                );
        (i64::from(placeable), Status::Ok)
    }

    /// A global rules reference property (appendix 7.8).
    pub(super) fn global_property(&self, name: &str) -> (i64, Status) {
        let session = &self.sim.session;
        let value = match name {
            "Frame" => i64::from(session.binary_frame()),
            "PlayerCount" => i64::from(crate::sim::crates::human_player_count(self.sim)),
            "GameSpeed" => i64::from(session.game_options().game_speed),
            "Difficulty" => i64::from(self.sim.trigger_difficulty()),
            "MissionTimer" => i64::from(session.campaign_countdown_frames()),
            "MissionTimerRunning" => i64::from(session.scenario_timer_view().is_some()),
            "AmbientLight" => i64::from(session.lighting().current_ambient()),
            "IonStormActive" => {
                i64::from(crate::sim::superweapon::lightning_storm::raging(self.sim))
            }
            // The room switch "Allow automation" (RFC 0001 6i, 10).
            "AutomationAllowed" => i64::from(self.sim.ext_modules.automation_allowed),
            _ => return (0, Status::Unsupported),
        };
        (value, Status::Ok)
    }

    /// `find` over cells, in cell index order (y, then x). A query with an
    /// owner answers `Unsupported`; its type, a TechnoType, is the one
    /// `Placeable` places.
    pub(super) fn find_cells(
        &self,
        query: &Query,
        cap: usize,
    ) -> Result<(FindPage, Status), Invalid> {
        if query.owner != 0 {
            return Ok((FindPage::default(), Status::Unsupported));
        }
        match decode_ref(query.type_ref)? {
            None => {}
            Some(Ref::Type {
                kind: NAME_TECHNO_TYPE,
                id,
            }) if i32::try_from(id).is_ok_and(|id| techno_type(self.rules, id).is_some()) => {}
            Some(_) => {
                return Err(Invalid(format!(
                    "{:#x} is not a TechnoType for a Cells query",
                    query.type_ref
                )));
            }
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
        // A later page starts at its key instead of walking up to it.
        let (after_y, after_x) = (query.after >> 16, query.after & 0xFFFF);
        'rows: for y in y0.max(after_y)..y1 {
            let x0 = if y == after_y { x0.max(after_x) } else { x0 };
            for x in x0..x1 {
                let key = (y << 16) | x;
                if page.found.len() == cap {
                    break 'rows;
                }
                let (Ok(rx), Ok(ry)) = (u16::try_from(x), u16::try_from(y)) else {
                    continue;
                };
                // Off the map, or (for a client) a cell it cannot see.
                if terrain.cell(rx, ry).is_none()
                    || self.gate.is_some_and(|gate| !gate.lists(cell_ref(rx, ry)))
                {
                    if !page.skip(key) {
                        break 'rows;
                    }
                    continue;
                }
                if !page.examine(key, query) {
                    break 'rows;
                }
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
