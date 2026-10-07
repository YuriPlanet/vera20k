//! Input-time Building443860 rally search; decoded event1E stores its result.
//! The input domain borrows current map state and isolates the fallback Cell.

use super::Simulation;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::FactoryType;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::components::DriveCoord;
use crate::sim::find_nearby_cell::{
    NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell,
    map_owned_radius_cap,
};
use crate::sim::occupancy::RawCellOccupationGrid;
use crate::sim::pathfinding::zone_map::ZoneGrid;

impl Simulation {
    /// BuildingClass::SetRallyPoint443860 prepares each factory's cell before
    /// enqueueing event1E. Foot/Normal are literal defaults even for a tank
    /// factory. AircraftType changes them to Winged/Fly; Naval overrides both.
    /// Source zone uses physical Location, and the clicked cell's bridge flag.
    /// Native FNPC selection reads binary frame; it makes no RNG draw.
    pub(crate) fn factory_rally_cell_input(
        &self,
        id: u64,
        clicked: (u16, u16),
        rules: &RuleSet,
    ) -> Result<Option<(u16, u16)>, String> {
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(None);
        };
        let Some(object) = rules.object(self.interner.resolve(entity.type_ref())) else {
            return Ok(None);
        };
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("rally input lacks terrain")?;
        let zones = self
            .zone_grid
            .as_ref()
            .ok_or("rally input lacks native zones")?;
        let bounds = self
            .playfield_bounds
            .ok_or("rally input lacks playfield bounds")?;
        let size = self
            .playfield_size_height
            .map(|height| (bounds.base, height))
            .or_else(|| self.bridge_state.as_ref()?.native_zone_source_size())
            .ok_or("rally input lacks native map dimensions")?;
        let cells = NativeCellQuery::isolated(terrain);
        resolve_rally_cell_click(
            &cells,
            zones,
            &self.substrate.raw_cell_occupation,
            bounds,
            size,
            self.session.binary_frame,
            &RallyCellClick {
                clicked: (clicked.0 as i16, clicked.1 as i16),
                location: crate::sim::movement::ground_pose::position_world_coord(&entity.position),
                factory: object.factory,
                naval: object.naval,
            },
        )
        // A miss sends no rally event and keeps ArchiveTarget. The separate
        // redeployable ConstructionYard branch requires its repack/move owner;
        // it is not admitted by the current HasRallyPoint cell-click route.
    }
}

struct RallyCellClick {
    clicked: (i16, i16),
    location: DriveCoord,
    factory: Option<FactoryType>,
    naval: bool,
}

/// Original4438B8..44395C literal caller arguments, using the existing
/// Map56D230 zone and Map56DC20 FNPC owners rather than a second path search.
#[allow(clippy::too_many_arguments)]
fn resolve_rally_cell_click(
    cells: &NativeCellQuery<'_>,
    zones: &ZoneGrid,
    raw: &RawCellOccupationGrid,
    bounds: PlayfieldBounds,
    size: (i32, i32),
    frame: u32,
    click: &RallyCellClick,
) -> Result<Option<(u16, u16)>, String> {
    let (speed_type, movement_zone) = if click.naval {
        (SpeedType::Amphibious, MovementZone::AmphibiousCrusher)
    } else if click.factory == Some(FactoryType::AircraftType) {
        (SpeedType::Winged, MovementZone::Fly)
    } else {
        (SpeedType::Foot, MovementZone::Normal)
    };
    let terrain = cells.terrain();
    let bridge = cells.flags(cells.lookup(click.clicked)) & 0x100 != 0;
    let source = (
        (click.location.x / 256) as i16 as u16,
        (click.location.y / 256) as i16 as u16,
    );
    let required_zone_id = zones
        .get_zone_id_native_in_query(terrain, source, movement_zone, bridge, Some(cells))
        .ok_or("rally FNPC source lacks native zone topology")?;
    Ok(find_nearby_passable_cell(
        (i32::from(click.clicked.0), i32::from(click.clicked.1)),
        &NearbyQuery {
            native_cells: Some(cells),
            raw_occupation: Some(raw),
            passability: PassabilityArgs {
                speed_type,
                required_zone_id: Some(required_zone_id),
                movement_zone,
                bridge_aware_zone: bridge,
            },
            footprint: NearbyFootprint::SINGLE,
            anchor_gate: NearbyAnchorGate::NativeHeightAware,
            allow_bridge_cells: true,
            check_height: false,
            check_occupancy: false,
            radius_cap: map_owned_radius_cap(size.0, size.1),
            target_cell: None,
            path_grid: None,
            resolved_terrain: Some(terrain),
            overlay_grid: None,
            occupancy: None,
            entities: None,
            zone_grid: Some(zones),
            playfield_bounds: Some(bounds),
        },
        frame,
    )
    .filter(|cell| *cell != (0, 0)))
}

#[cfg(test)]
mod tests {
    use super::super::native_cell_input_test_fixture::{bounds, coord, native_fixture, pair};
    use super::*;
    use crate::map::cell_index::NativeCellIdentity;
    use serde_json::{Value, json};

    #[test]
    fn prepared_rally_cell_reaches_archive_and_stop_without_input_mutation() {
        use crate::map::entities::EntityCategory;
        use crate::rules::ini_parser::IniFile;
        use crate::sim::combat::TargetKind;
        use crate::sim::command::Command;
        use crate::sim::components::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::util::fixed_math::SimFixed;
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/rally_input.json",
        ))
        .unwrap();
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=FACTORY\n[FACTORY]\nFactory=UnitType\nStrength=1000\n",
        ))
        .unwrap();
        for name in ["clicked_raw_0000001f", "no_passable_speed"] {
            let row = corpus["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["input"]["name"] == name)
                .unwrap();
            let (terrain, zones, raw) = native_fixture(&row["input"]);
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Local");
            let type_ref = sim.interner.intern("FACTORY");
            let mut actor = GameEntity::new_at_frame_zero_for_test(
                1,
                5,
                5,
                0,
                0,
                owner,
                Health { current: 1000 },
                type_ref,
                EntityCategory::Structure,
                0,
                5,
                false,
            );
            actor.lifecycle.in_limbo = false;
            actor.position.sub_x = SimFixed::from_num(64);
            actor.position.sub_y = SimFixed::from_num(64);
            actor.set_archive_target(Some(TargetKind::Cell(6, 5)));
            sim.substrate.entities.insert(actor);
            sim.resolved_terrain = Some(terrain);
            sim.zone_grid = Some(zones);
            sim.substrate.raw_cell_occupation = raw;
            sim.playfield_bounds = Some(bounds());
            sim.playfield_size_height = Some(8);
            sim.session.binary_frame = 100;
            let rng = sim.scenario_rng.state();
            let prepared = sim.factory_rally_cell_input(1, (11, 5), &rules).unwrap();
            let expected = pair(&row["nearby"][0]["result"]);
            assert_eq!(prepared, (expected != (0, 0)).then_some(expected), "{name}");
            assert_eq!(
                sim.substrate.entities.get(1).unwrap().archive_target(),
                Some(TargetKind::Cell(6, 5))
            );
            assert_eq!(
                sim.resolved_terrain
                    .as_ref()
                    .unwrap()
                    .dummy_cell_requested_coord(),
                (99, 98)
            );
            assert_eq!(sim.scenario_rng.state(), rng);
            if let Some((rx, ry)) = prepared {
                let command = Command::SetRally {
                    rx,
                    ry,
                    producer_ids: vec![1],
                };
                let command: Command =
                    serde_json::from_slice(&serde_json::to_vec(&command).unwrap()).unwrap();
                assert!(sim.apply_command("Local", &command, Some(&rules)));
                assert_eq!(
                    sim.substrate.entities.get(1).unwrap().archive_target(),
                    Some(TargetKind::Cell(expected.0, expected.1))
                );
                assert!(sim.apply_command("Local", &Command::Stop { entity_id: 1 }, Some(&rules)));
                assert_eq!(
                    sim.substrate.entities.get(1).unwrap().archive_target(),
                    None
                );
            }
        }
    }

    /// Whole original443860 corpus supplies expected FNPC results. Queue-full
    /// controls still search: transport admission belongs after this resolver.
    #[test]
    fn rally_cell_input_matches_25_defined_original_building_clicks() {
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/rally_input.json",
        ))
        .unwrap();
        let rows = corpus["cases"].as_array().unwrap();
        assert_eq!(rows.len(), 27);
        let mut compared = 0;
        for row in rows {
            let input = &row["input"];
            // Original56E6AF/CDQ+IDIV can index BEFORE its candidate array for
            // negative Frame. The zeroed native fixture's return is then stack
            // history, not a defined golden. Keep both controls in the evidence
            // without claiming the portable unsigned extension matches them.
            if input["frame"].as_u64().unwrap_or(100) > i32::MAX as u64 {
                continue;
            }
            compared += 1;
            let (terrain, zones, raw) = native_fixture(input);
            let cells = NativeCellQuery::isolated(&terrain);
            let clicked = pair(input.get("clicked").unwrap_or(&json!([11, 5])));
            let result = resolve_rally_cell_click(
                &cells,
                &zones,
                &raw,
                bounds(),
                (8, 8),
                input["frame"].as_u64().unwrap_or(100) as u32,
                &RallyCellClick {
                    clicked: (clicked.0 as i16, clicked.1 as i16),
                    location: coord(input.get("source").unwrap_or(&json!([1344, 1344, 0]))),
                    factory: Some(match input["factory"].as_u64().unwrap_or(40) {
                        40 => FactoryType::UnitType,
                        16 => FactoryType::InfantryType,
                        3 => FactoryType::AircraftType,
                        other => panic!("unmapped native Factory {other}"),
                    }),
                    naval: input["naval"].as_bool().unwrap_or(false),
                },
            )
            .unwrap();
            let expected = pair(&row["nearby"][0]["result"]);
            assert_eq!(result, (expected != (0, 0)).then_some(expected), "{input}");
            let dummy = pair(&row["nearby"][0]["dummy_at_return"]);
            assert_eq!(
                cells.coord(NativeCellIdentity::Dummy),
                (dummy.0 as i16, dummy.1 as i16),
                "{input}"
            );
            assert_eq!(terrain.dummy_cell_requested_coord(), (99, 98), "{input}");
        }
        assert_eq!(compared, 25);
    }
}
