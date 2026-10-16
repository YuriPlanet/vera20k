//! Where `BuildingClass::ExitObject_Main @ 0x00443C60` releases a produced
//! Unit or Infantry: the arm its producer's type picks, and the Cell or
//! coordinate each arm places it at. `production_queue::exit_produced_object`
//! runs the arms.

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::{FactoryType, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;

/// The arm of `BuildingClass::ExitObject_Main @ 0x00443C60` that releases a
/// produced Unit or Infantry. The producer's BuildingType alone picks it
/// (`0x0044413D..0x00444632`), whatever the product's class; only Aircraft
/// and Building products take arms of their own, earlier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExitArm {
    /// `Refinery=` (`+0x16BB`) or `Weeder=` (`+0x16BC`): `0x00444DE4`.
    Refinery,
    /// `WeaponsFactory=` (`+0x16BD`) and `Naval=` (`+0xCCE`): `0x00444175`.
    Naval,
    /// `WeaponsFactory=` without `Naval=`: `0x00444492`.
    Land,
    /// `Factory=InfantryType` (`+0xEB8`), `Hospital=` (`+0x16C1`), `Armory=`
    /// (`+0x16C2`) or `Cloning=` (`+0x16AC`): `0x0044498E`.
    Infantry,
    /// Any other producer: `0x00444638`.
    Generic,
}

/// The [`ExitArm`] a product of `producer` leaves through.
pub(super) fn exit_arm(producer: &ObjectType) -> ExitArm {
    if producer.refinery || producer.weeder {
        ExitArm::Refinery
    } else if producer.weapons_factory {
        if producer.naval {
            ExitArm::Naval
        } else {
            ExitArm::Land
        }
    } else if producer.factory_type() == Some(FactoryType::InfantryType)
        || producer.hospital
        || producer.armory()
        || producer.cloning
    {
        ExitArm::Infantry
    } else {
        ExitArm::Generic
    }
}

/// Where the Infantry and generic arms place their object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DockExit {
    /// The Unlimbo coordinate.
    pub coordinate: crate::sim::components::DriveCoord,
    /// The Unlimbo facing.
    pub facing: u8,
    /// The dock Cell, which each arm assigns as the object's destination.
    pub cell: (u16, u16),
}

/// The placement of the Infantry and generic arms: the producer's
/// GetDockCellForObject (vt+0x4D4, `0x0044EFB0`) for `mover`, offered the
/// zero Cell, then [`dock_exit_coordinate_and_facing`] for that Cell. The
/// Infantry arm's call (`0x004449AF`) offers the empty Cell (`0x0089C818`,
/// zeroed by `0x0043B0E0`); the generic arm's (`0x0044464D`) offers
/// ExitObject's own Cell argument, which its callers pass zero (Factory_AI
/// `0x00450155`, Place_Production `0x004FB563`, the Cloning Vat loop
/// `0x00444A3E`). `None` is the empty dock Cell, on which both arms answer 0
/// (`0x00445696`).
pub(super) fn dock_exit(
    sim: &Simulation,
    rules: &RuleSet,
    producer_id: u64,
    mover: u64,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<DockExit> {
    let cell = crate::sim::docking::building_dock::building_dock_cell(
        sim,
        producer_id,
        mover,
        (0, 0),
        rules,
        overlay_registry,
    )?;
    let producer = sim.substrate.entities.get(producer_id)?;
    let producer_type = sim.object_type(producer.type_ref(), rules)?;
    let (coordinate, facing) =
        dock_exit_coordinate_and_facing(producer, producer_type, (cell.0 as i16, cell.1 as i16));
    Some(DockExit {
        coordinate,
        facing,
        cell,
    })
}

/// The Unlimbo coordinate and facing at dock Cell `dock`, which the
/// Infantry (`0x00444A53..0x00444C7C`) and generic (`0x00444677..0x004448A7`)
/// arms compute alike. The angle uses the unshrunk dock centre and
/// Building447AC0's foundation centre; each outside component moves one
/// cell inward before physical Unlimbo. Exact preferred cells alone add
/// their type's rules-side ExitCoord.
/// Native goldens: basic-factory-exit-geometry-research/geometry-primary.json,
/// observation570630adeb29995ecc943d42d84f7659d19a02ae8c55ac5296b499e637ce87ba.
fn dock_exit_coordinate_and_facing(
    producer: &crate::sim::game_entity::GameEntity,
    producer_type: &crate::rules::object_type::ObjectType,
    dock: (i16, i16),
) -> (crate::sim::components::DriveCoord, u8) {
    use crate::sim::{components::DriveCoord, movement::ground_pose};
    let location = ground_pose::position_world_xy(&producer.position);
    let nw = (
        crate::util::lepton::lepton_to_cell_packed(location[0]),
        crate::util::lepton::lepton_to_cell_packed(location[1]),
    );
    let centre = |cell: i16| (i32::from(cell) << 8).wrapping_add(128);
    let word = crate::util::direction_tables::facing16_between(
        ground_pose::object_center_xy(producer),
        [centre(dock.0), centre(dock.1)],
    );
    let facing = crate::util::direction_tables::quantize::round_facing16_to_8(word);
    let (width, height) =
        crate::rules::foundation::foundation_dimensions(producer.type_facts.foundation());
    let inward = |cell: i16, base: i16, size: u16| {
        if i32::from(cell) >= i32::from(base) + i32::from(size) {
            cell.wrapping_sub(1)
        } else if cell < base {
            cell.wrapping_add(1)
        } else {
            cell
        }
    };
    let mut coordinate = DriveCoord {
        x: centre(inward(dock.0, nw.0, width)),
        y: centre(inward(dock.1, nw.1, height)),
        z: 0,
    };
    if (producer_type.gdi_barracks() && dock == (nw.0.wrapping_add(1), nw.1.wrapping_add(2)))
        || (producer_type.nod_barracks() && dock == (nw.0.wrapping_add(2), nw.1.wrapping_add(2)))
        || (producer_type.yuri_barracks() && dock == (nw.0.wrapping_add(2), nw.1.wrapping_add(1)))
    {
        let (x, y, z) = producer_type.exit_coord.unwrap_or((0, 0, 0));
        coordinate.x = coordinate.x.wrapping_add(x);
        coordinate.y = coordinate.y.wrapping_add(y);
        coordinate.z = coordinate.z.wrapping_add(z);
    }
    (coordinate, facing)
}
/// The land arm's HELLO (`0x004445D6`) and TETHER (`0x004445E3`) from the
/// producer to its product, through the shared radio owners, which own the
/// reciprocal contacts, flags and their cleanup. No class test precedes
/// them, and their answers are ignored.
pub fn mark_war_factory_spawn_contact(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer_id: u64,
    produced_id: u64,
    effects: FrameEffects<'_>,
) {
    for message in [
        crate::sim::radio::RadioMessage::Hello,
        crate::sim::radio::RadioMessage::Tether,
    ] {
        crate::sim::radio::transmit(
            sim,
            producer_id,
            produced_id,
            message,
            crate::sim::radio::RadioPayload::default(),
            Some(rules),
            effects,
        );
    }
}

/// The naval arm's Cell (`0x00444175..0x004443DC`) for a product of
/// `speed_type`: [`find_naval_unit_delivery_cell`] from the producer.
pub(super) fn naval_exit_cell(
    sim: &Simulation,
    producer_id: u64,
    speed_type: SpeedType,
) -> Option<(u16, u16)> {
    let producer = sim.substrate.entities.get(producer_id)?;
    let map_size = sim
        .playfield_bounds
        .zip(sim.playfield_size_height)
        .map(|(bounds, height)| (bounds.base, height));
    let (cell, _) = find_naval_unit_delivery_cell(
        producer_id,
        producer.position.rx(),
        producer.position.ry(),
        producer.rally_cell(),
        speed_type,
        sim.path_grid(),
        &sim.substrate.occupancy,
        &sim.substrate.entities,
        sim.resolved_terrain.as_ref(),
        sim.overlay_grid.as_ref(),
        sim.zone_grid.as_ref(),
        // The frame counter is committed late: during this advance it holds
        // the current frame, the value FNPC's pick must alias.
        sim.session.binary_frame,
        sim.playfield_bounds,
        map_size,
    );
    Some(cell)
}

/// Active-retail produced-Unit naval delivery owner.
///
/// `BuildingClass::ExitObject_Main @ 0x00443C60` begins at the producer's
/// `GetCoords` foundation centre, optionally walks out of the producer toward
/// its own ArchiveTarget/rally cell, and otherwise invokes the one shared FNPC
/// call at `0x004443DC`. FNPC failure is the literal zero cell; the caller still
/// performs one normal Unlimbo attempt against that result.
fn find_naval_unit_delivery_cell(
    producer_id: u64,
    base_rx: u16,
    base_ry: u16,
    producer_rally: Option<(u16, u16)>,
    speed_type: SpeedType,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    zone_grid: Option<&crate::sim::pathfinding::zone_map::ZoneGrid>,
    frame_counter: u32,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    map_size: Option<(i32, i32)>,
) -> ((u16, u16), bool) {
    let Some(origin) = producer_get_coords_cell(entities, producer_id) else {
        return ((0, 0), false);
    };

    if let Some(rally) = producer_rally
        && let Some(candidate) = naval_rally_fast_path_cell(
            producer_id,
            (base_rx, base_ry),
            origin,
            rally,
            occupancy,
            entities,
            resolved_terrain,
            playfield_bounds,
        )
    {
        return (candidate, true);
    }

    let Some(grid) = path_grid else {
        return ((0, 0), false);
    };
    let Some(query) = nearby_query_for_naval_unit_delivery(
        speed_type,
        grid,
        resolved_terrain,
        overlay_grid,
        zone_grid,
        playfield_bounds,
        map_size,
    ) else {
        return ((0, 0), false);
    };
    (
        crate::sim::find_nearby_cell::find_nearby_passable_cell(
            (i32::from(origin.0), i32::from(origin.1)),
            &query,
            frame_counter,
        )
        .unwrap_or((0, 0)),
        false,
    )
}

/// The producing building's GetCoords (`BuildingClass::GetCoords @
/// 0x00447AC0`) as a cell (`ObjectClass::Get_Cell_Packed @ 0x0041BEA0`).
fn producer_get_coords_cell(entities: &EntityStore, producer_id: u64) -> Option<(u16, u16)> {
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entities.get(producer_id)?);
    let cell = |value: i32| u16::try_from(crate::util::lepton::lepton_to_cell_packed(value)).ok();
    cell(x).zip(cell(y))
}

fn naval_rally_fast_path_cell(
    producer_id: u64,
    producer_nw: (u16, u16),
    foundation_center: (u16, u16),
    rally: (u16, u16),
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
) -> Option<(u16, u16)> {
    let candidate = naval_rally_walk_candidate(
        producer_id,
        producer_nw,
        foundation_center,
        rally,
        occupancy,
    );

    let terrain = resolved_terrain?;
    if terrain.cell(candidate.0, candidate.1)?.yr_cell_land_type
        != crate::rules::terrain_rules::LandType::Water.as_index()
    {
        return None;
    }
    let has_eligible_active_object = occupancy
        .get(candidate.0, candidate.1)
        .into_iter()
        .flat_map(|cell| cell.iter_layer(MovementLayer::Ground))
        .any(|occupant| {
            entities
                .get(occupant.entity_id)
                .is_some_and(|entity| entity.is_active())
        });
    if has_eligible_active_object {
        return None;
    }
    crate::sim::cell_rect::cell_is_in_playfield_height_aware(
        (i32::from(candidate.0), i32::from(candidate.1)),
        playfield_bounds,
        Some(terrain),
    )
    .then_some(candidate)
}

fn naval_rally_walk_candidate(
    producer_id: u64,
    producer_nw: (u16, u16),
    foundation_center: (u16, u16),
    rally: (u16, u16),
    occupancy: &OccupancyGrid,
) -> (u16, u16) {
    let facing = crate::util::fixed_math::facing_from_delta_int(
        i32::from(rally.0) - i32::from(producer_nw.0),
        i32::from(rally.1) - i32::from(producer_nw.1),
    );
    let (step_x, step_y) = crate::util::fixed_math::dir_to_cell_delta(facing);
    let mut candidate = foundation_center;

    // Look_up_building_in_cell @ 0x0047C520 returns the first Building in
    // literal +0xE4 list order. A different first Building stops the walk even
    // if this producer occurs later in the same cell's list.
    while occupancy.first_building_on_layer(candidate.0, candidate.1, MovementLayer::Ground)
        == Some(producer_id)
    {
        candidate = (
            candidate.0.wrapping_add_signed(step_x as i16),
            candidate.1.wrapping_add_signed(step_y as i16),
        );
    }
    candidate
}

fn nearby_query_for_naval_unit_delivery<'a>(
    speed_type: SpeedType,
    grid: &'a crate::sim::pathfinding::PathGrid,
    resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    overlay_grid: Option<&'a crate::sim::overlay_grid::OverlayGrid>,
    zone_grid: Option<&'a crate::sim::pathfinding::zone_map::ZoneGrid>,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    map_size: Option<(i32, i32)>,
) -> Option<crate::sim::find_nearby_cell::NearbyQuery<'a>> {
    use crate::sim::find_nearby_cell::{
        NearbyFootprint, NearbyQuery, PassabilityArgs, map_owned_radius_cap,
    };
    let (size_width, size_height) = map_size?;
    Some(NearbyQuery {
        native_cells: None,
        raw_occupation: None,
        passability: PassabilityArgs {
            // Unit vtable +0x84 -> +0x88 -> UnitType+0x67C.
            speed_type,
            required_zone_id: None,
            // Literal caller argument 5 at 0x0044434B..0x004443DC.
            movement_zone: MovementZone::Normal,
            bridge_aware_zone: false,
        },
        footprint: NearbyFootprint::SINGLE,
        allow_bridge_cells: true,
        check_height: false,
        // Arguments 11 and 15 are both false. Immediate UnitClass::Unlimbo,
        // not FNPC, owns the one occupancy/CanEnter/Mark decision.
        check_occupancy: false,
        radius_cap: map_owned_radius_cap(size_width, size_height),
        // The caller passes a pointer to CellStruct(0,0); FNPC maps that value
        // to live-frame modulo selection with no RNG draw.
        target_cell: None,
        path_grid: Some(grid),
        resolved_terrain,
        overlay_grid,
        // Both caller occupancy flags are false, so this query must not let
        // CellRect's compatibility object-list projection recreate one.
        occupancy: None,
        entities: None,
        zone_grid,
        playfield_bounds,
    })
}

/// Naval ExitObject's selected Cell goes through the same concrete Unlimbo
/// boundary as every Unit constructor; there is no production +1AC port.
pub(super) fn unlimbo_held_naval_unit(
    sim: &mut Simulation,
    rules: &RuleSet,
    stable_id: u64,
    cell: (u16, u16),
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> Option<u64> {
    // The selected Cell's GetCoords (`0x00486840`).
    let coord = crate::sim::movement::target_cell_coord(
        cell.0,
        cell.1,
        sim.resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
            .as_ref(),
    );
    sim.reveal_constructed_object_at_coord_with_overlay_context(
        stable_id,
        coord,
        0x40,
        crate::sim::world::PlacementEvidence::EvaluateMark,
        rules,
        overlay_registry,
        effects,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainGrid, zone_class};
    use crate::rules::terrain_rules::TerrainClass;
    use crate::sim::entity_store::EntityStore;
    use crate::sim::pathfinding::PathGrid;

    fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(
            width,
            height,
            crate::map::resolved_terrain::test_clear_cell,
        )
    }

    #[test]
    fn dock_exit_coordinate_and_facing_match_original_instructions() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::components::DriveCoord;
        use crate::sim::game_entity::GameEntity;
        let data: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "src/sim/production/fixtures/infantry_exit_native.json",
        ))
        .unwrap();
        assert_eq!(
            data["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
        let mut compared = 0;
        let mut storage_residuals = 0;
        for row in data["rows"].as_array().unwrap() {
            let number = |key: &str, index: usize| row[key][index].as_i64().unwrap() as i32;
            let location = DriveCoord {
                x: number("location", 0),
                y: number("location", 1),
                z: number("location", 2),
            };
            if location.x == i32::MIN || location.x == i32::MAX {
                storage_residuals += 1;
                continue;
            }
            let rules = RuleSet::from_ini_with_fixed_art_for_test(
                &IniFile::from_str(&format!(
                    "[BuildingTypes]\n0=GAPILE\n[GAPILE]\nGDIBarracks={}\nNODBarracks={}\nYuriBarracks={}\nExitCoord={},{},{}\n",
                    if number("flags", 0) != 0 { "yes" } else { "no" },
                    if number("flags", 1) != 0 { "yes" } else { "no" },
                    if number("flags", 2) != 0 { "yes" } else { "no" },
                    number("exit_coord", 0), number("exit_coord", 1), number("exit_coord", 2),
                )),
                &IniFile::from_str("[GAPILE]\nFoundation=3x2\n"),
            ).unwrap();
            let mut producer = GameEntity::test_default_of_category(
                1,
                "GAPILE",
                "Americans",
                14,
                14,
                crate::map::entities::EntityCategory::Structure,
            );
            producer.type_facts.set_foundation_for_test("3x2".into());
            producer.position.put_location(location);
            let dock = (number("dock", 0) as i16, number("dock", 1) as i16);
            let producer_type = rules.object("GAPILE").unwrap();
            assert_eq!(
                [
                    producer_type.gdi_barracks(),
                    producer_type.nod_barracks(),
                    producer_type.yuri_barracks()
                ],
                [
                    number("flags", 0) != 0,
                    number("flags", 1) != 0,
                    number("flags", 2) != 0
                ],
                "{}: fixture preserves each native nonzero-byte predicate",
                row["name"]
            );
            let (coordinate, facing) =
                dock_exit_coordinate_and_facing(&producer, producer_type, dock);
            assert_eq!(
                [coordinate.x, coordinate.y, coordinate.z],
                [
                    number("coordinate", 0),
                    number("coordinate", 1),
                    number("coordinate", 2)
                ],
                "{}",
                row["name"]
            );
            assert_eq!(
                i32::from(facing),
                row["facing"].as_i64().unwrap() as i32,
                "{}",
                row["name"]
            );
            assert_eq!(
                crate::util::direction_tables::quantize::round_facing16_to_8(
                    row["facing_word"][0].as_u64().unwrap() as u16,
                ),
                facing,
                "{}",
                row["name"]
            );
            compared += 1;
        }
        assert_eq!(compared, 26);
        assert_eq!(storage_residuals, 2);
    }

    fn test_playfield_bounds() -> crate::sim::cell_rect::PlayfieldBounds {
        crate::sim::cell_rect::PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        }
    }

    fn naval_delivery_rules() -> RuleSet {
        RuleSet::from_ini_with_fixed_art_for_test(
            &crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=DEST\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAYARD\n\
             1=BLOCKER\n\
             [DEST]\n\
             Strength=600\n\
             Speed=6\n\
             SpeedType=Float\n\
             MovementZone=Water\n\
             Locomotor={2BEA74E1-7CCA-11D3-BE14-00104B62A16C}\n\
             [GAYARD]\n\
             Factory=UnitType\n\
             WeaponsFactory=yes\n\
             Naval=yes\n\
             Foundation=4x4\n\
             [BLOCKER]\n\
             Foundation=1x1\n",
            ),
            &crate::rules::ini_parser::IniFile::from_str(
                "[GAYARD]\nFoundation=4x4\n[BLOCKER]\nFoundation=1x1\n",
            ),
        )
        .expect("naval delivery rules")
    }

    fn production_admission_rules() -> RuleSet {
        RuleSet::from_ini_with_fixed_art_for_test(
            &crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=SAPC\n\
             1=DEST\n\
             2=RANKER\n\
             3=ARMED\n\
             4=OMNI\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=YARD\n\
             1=INVISIBLE\n\
             2=BIBBER\n\
             3=NORMAL\n\
             4=GATE\n\
             5=CABHUT\n\
             6=LASERDEFAULT\n\
             7=FIREDEFAULT\n\
             [SAPC]\n\
             SpeedType=Hover\n\
             Crusher=yes\n\
             [DEST]\n\
             SpeedType=Float\n\
             [RANKER]\n\
             SpeedType=Hover\n\
             VeteranAbilities=CRUSHER\n\
             [ARMED]\n\
             SpeedType=Hover\n\
             Primary=CANNON\n\
             [OMNI]\n\
             SpeedType=Hover\n\
             OmniCrusher=yes\n\
             [YARD]\n\
             Foundation=4x4\n\
             UnitRepair=yes\n\
             NumberImpassableRows=3\n\
             [INVISIBLE]\n\
             InvisibleInGame=yes\n\
             [BIBBER]\n\
             Bib=yes\n\
             [NORMAL]\n\
             Foundation=1x1\n\
             [GATE]\n\
             Gate=yes\n\
             DamagedDoor=yes\n\
             [CABHUT]\n\
             BridgeRepairHut=yes\n\
             [LASERDEFAULT]\n\
             LaserFence=yes\n\
             [FIREDEFAULT]\n\
             Foundation=1x1\n",
            ),
            &crate::rules::ini_parser::IniFile::from_str(
                "[YARD]\nFoundation=4x4\n[NORMAL]\nFoundation=1x1\n[FIREDEFAULT]\nFoundation=1x1\n",
            ),
        )
        .expect("production admission rules")
    }

    fn production_admission_sim() -> Simulation {
        let mut sim = Simulation::default();
        let mut terrain = flat_terrain(20, 20);
        for cell in &mut terrain.cells {
            // Retail ground rows used by the compact admission matrix.
            cell.speed_costs.float = Some(0);
            cell.base_speed_costs.float = Some(0);
            cell.speed_costs.hover = Some(50);
            cell.base_speed_costs.hover = Some(50);
        }
        sim.resolved_terrain = Some(terrain);
        sim.playfield_bounds = Some(test_playfield_bounds());
        sim
    }

    fn set_admission_water_cell(sim: &mut Simulation, cell: (u16, u16)) {
        let water = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        water.yr_cell_land_type = crate::rules::terrain_rules::LandType::Water.as_index();
        water.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
        water.terrain_class = TerrainClass::Water;
        water.speed_costs.float = Some(100);
        water.base_speed_costs.float = Some(100);
    }

    fn water_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let mut terrain = flat_terrain(width, height);
        for cell in &mut terrain.cells {
            cell.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
            cell.yr_cell_land_type = crate::rules::terrain_rules::LandType::Water.as_index();
            cell.base_land_type = crate::rules::terrain_rules::LandType::Water.as_index();
            cell.base_yr_cell_land_type = crate::rules::terrain_rules::LandType::Water.as_index();
            cell.terrain_class = TerrainClass::Water;
            cell.base_terrain_class = TerrainClass::Water;
            cell.is_water = true;
            cell.zone_type = zone_class::WATER;
            cell.speed_costs.float = Some(100);
            cell.base_speed_costs.float = Some(100);
        }
        terrain
    }

    fn test_entity(
        stable_id: u64,
        category: crate::map::entities::EntityCategory,
        cell: (u16, u16),
    ) -> crate::sim::game_entity::GameEntity {
        let mut entity = crate::sim::game_entity::GameEntity::test_default(
            stable_id,
            if category == crate::map::entities::EntityCategory::Structure {
                "BLOCKER"
            } else {
                "DEST"
            },
            "Americans",
            cell.0,
            cell.1,
        );
        entity.set_category_for_test(category);
        entity
    }

    /// Producer 10: the `naval_delivery_rules` GAYARD at NW cell (10,10),
    /// with the `Foundation=4x4` construction stamps on it.
    fn gayard_producer() -> crate::sim::game_entity::GameEntity {
        let mut producer =
            crate::sim::game_entity::GameEntity::test_default(10, "GAYARD", "Americans", 10, 10);
        producer.set_category_for_test(crate::map::entities::EntityCategory::Structure);
        producer
            .type_facts
            .set_foundation_for_test("4x4".to_string());
        producer
    }

    /// `0x0044413D..0x00444632` tests Refinery, Weeder, WeaponsFactory and
    /// Naval, then Factory=InfantryType, Hospital, Armory and Cloning, each
    /// ahead of the next; nothing else in the type picks an arm.
    #[test]
    fn exit_arm_reads_the_producer_flags_in_native_order() {
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             0=REFWEAP\n1=WEEDYARD\n2=YARD\n3=WEAP\n4=WEAPBARR\n5=BARR\n6=HOSP\n\
             7=ARMORY\n8=VATS\n9=UNITFAC\n10=AIRFAC\n11=PLAIN\n\
             [REFWEAP]\nRefinery=yes\nWeaponsFactory=yes\n\
             [WEEDYARD]\nWeeder=yes\nWeaponsFactory=yes\nNaval=yes\n\
             [YARD]\nWeaponsFactory=yes\nNaval=yes\nFactory=InfantryType\n\
             [WEAP]\nWeaponsFactory=yes\nFactory=UnitType\n\
             [WEAPBARR]\nWeaponsFactory=yes\nFactory=InfantryType\n\
             [BARR]\nFactory=InfantryType\nNaval=yes\n\
             [HOSP]\nHospital=yes\n\
             [ARMORY]\nArmory=yes\n\
             [VATS]\nCloning=yes\n\
             [UNITFAC]\nFactory=UnitType\nExitCoord=512,256,0\n\
             [AIRFAC]\nFactory=AircraftType\n\
             [PLAIN]\nGDIBarracks=yes\n",
        ))
        .unwrap();
        for (producer, arm) in [
            ("REFWEAP", ExitArm::Refinery),
            ("WEEDYARD", ExitArm::Refinery),
            ("YARD", ExitArm::Naval),
            ("WEAP", ExitArm::Land),
            ("WEAPBARR", ExitArm::Land),
            ("BARR", ExitArm::Infantry),
            ("HOSP", ExitArm::Infantry),
            ("ARMORY", ExitArm::Infantry),
            ("VATS", ExitArm::Infantry),
            ("UNITFAC", ExitArm::Generic),
            ("AIRFAC", ExitArm::Generic),
            ("PLAIN", ExitArm::Generic),
        ] {
            assert_eq!(exit_arm(rules.object(producer).unwrap()), arm, "{producer}");
        }
    }

    /// Through the production reader on the retail INIs, every building
    /// that produces Units or Infantry for a house leaves them through the
    /// land, naval or Infantry arm; none reaches the refinery or generic arm.
    #[test]
    fn retail_producers_take_the_land_naval_and_infantry_arms() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let mut arms = Vec::new();
        for object in retail.rules.all_objects() {
            if !matches!(
                object.factory_type(),
                Some(FactoryType::UnitType | FactoryType::InfantryType)
            ) {
                continue;
            }
            let arm = exit_arm(object);
            assert!(
                matches!(arm, ExitArm::Naval | ExitArm::Land | ExitArm::Infantry),
                "{}: {arm:?}",
                object.id
            );
            if !arms.contains(&arm) {
                arms.push(arm);
            }
        }
        arms.sort_by_key(|arm| *arm as u8);
        assert_eq!(arms, [ExitArm::Naval, ExitArm::Land, ExitArm::Infantry]);
    }

    #[test]
    fn naval_exit_starts_at_the_getcoords_centre() {
        let rules = naval_delivery_rules();
        let yard = rules.object("GAYARD").unwrap();
        assert_eq!(exit_arm(yard), ExitArm::Naval);
        assert_eq!(yard.foundation, "4x4");
        let mut entities = EntityStore::new();
        entities.insert(gayard_producer());
        assert_eq!(
            producer_get_coords_cell(&entities, 10),
            Some((12, 12)),
            "4x4 NW (10,10) GetCoords adds 384 leptons then truncates to (12,12)"
        );
    }

    #[test]
    fn unit_unlimbo_native_empty_raw_byte_is_not_resurrected_by_owner_claims() {
        let rules = production_admission_rules();
        let mut sim = production_admission_sim();
        set_admission_water_cell(&mut sim, (8, 8));
        let stable_id = sim
            .create_production_object_limbo_at_height("DEST", "Americans", 8, 8, 0x40, 0, &rules)
            .unwrap();
        // Unit7441B0 ORs 0x20; Unit744210 destructively clears it. The
        // derived owner index can retain another claim after that clear.
        for owner in [101, 102] {
            sim.substrate
                .cell_occupation
                .mark_vehicle_on_layer(8, 8, owner, MovementLayer::Ground);
            sim.substrate.raw_cell_occupation.mark_ground(8, 8, 0x20);
        }
        sim.substrate
            .cell_occupation
            .clear_vehicle_on_layer(8, 8, 101, MovementLayer::Ground);
        sim.substrate.raw_cell_occupation.clear_ground(8, 8, 0x20);
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(8, 8), 0);
        assert_eq!(
            sim.substrate
                .cell_occupation
                .vehicle_bits(8, 8, MovementLayer::Ground),
            0x20
        );
        // Original Unit Unlimbo's empty-list/raw-zero control admits. Claims
        // are not another input of the native +1AC receiver.
        assert_eq!(
            unlimbo_held_naval_unit(
                &mut sim,
                &rules,
                stable_id,
                (8, 8),
                None,
                crate::sim::world::FrameEffects::default()
            ),
            Some(stable_id)
        );
    }

    #[test]
    fn unit_unlimbo_native_deck_query_keeps_the_callers_ground_pose() {
        let rules = production_admission_rules();
        let mut sim = production_admission_sim();
        set_admission_water_cell(&mut sim, (8, 8));
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(8, 8)
            .unwrap();
        cell.level = 3;
        cell.bridge_deck_level = 7;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_facts.raw_flags = 0x100;
        let stable_id = sim
            .create_production_object_limbo_at_height("DEST", "Americans", 8, 8, 0x40, 3, &rules)
            .unwrap();
        assert_eq!(
            unlimbo_held_naval_unit(
                &mut sim,
                &rules,
                stable_id,
                (8, 8),
                None,
                crate::sim::world::FrameEffects::default()
            ),
            Some(stable_id)
        );
        let entity = sim.substrate.entities.get(stable_id).unwrap();
        // Original Foot4D9C60 changes only caller-local query outputs;
        // initialized native bridge_clear_ground_input retains OnBridge0.
        assert!(!entity.on_bridge());
        assert_eq!(entity.position.exact_z_leptons(), Some(312));
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(8, 8) & 0x20,
            0x20
        );
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(8, 8) & 0x20, 0);
    }

    #[test]
    fn production_exact_zero_unlimbo_marks_held_identity_without_forced_outcome() {
        let rules = production_admission_rules();
        let mut sim = production_admission_sim();
        set_admission_water_cell(&mut sim, (8, 8));
        let stable_id = sim
            .create_production_object_limbo_at_height("DEST", "Americans", 8, 8, 0x40, 0, &rules)
            .expect("held production Unit");
        assert_eq!(
            unlimbo_held_naval_unit(
                &mut sim,
                &rules,
                stable_id,
                (8, 8),
                None,
                crate::sim::world::FrameEffects::default(),
            ),
            Some(stable_id),
            "the production API exposes no caller-forced Mark outcome"
        );
        let entity = sim.substrate.entities.get(stable_id).unwrap();
        assert!(!entity.lifecycle.in_limbo() && entity.lifecycle.cell_marked());
    }

    #[test]
    fn naval_fallback_query_uses_dynamic_speed_literal_zone_zero_and_live_frame_pool() {
        let mut terrain = water_terrain(7, 7);
        for cell in &mut terrain.cells {
            cell.speed_costs.float = Some(0);
            cell.base_speed_costs.float = Some(0);
        }
        for candidate in [(2, 1), (3, 1)] {
            let cell = terrain.cell_mut(candidate.0, candidate.1).unwrap();
            cell.speed_costs.float = Some(100);
            cell.base_speed_costs.float = Some(100);
        }
        let grid = PathGrid::from_resolved_terrain(&terrain);
        let query = nearby_query_for_naval_unit_delivery(
            SpeedType::Float,
            &grid,
            Some(&terrain),
            None,
            None,
            Some(test_playfield_bounds()),
            Some((7, 7)),
        )
        .unwrap();

        assert_eq!(query.passability.speed_type, SpeedType::Float);
        assert_eq!(query.passability.movement_zone, MovementZone::Normal);
        assert_eq!(query.passability.required_zone_id, None);
        assert!(!query.passability.bridge_aware_zone);
        assert_eq!(
            query.footprint,
            crate::sim::find_nearby_cell::NearbyFootprint::SINGLE
        );
        assert!(query.allow_bridge_cells);
        assert!(!query.check_height && !query.check_occupancy);
        assert_eq!(query.target_cell, None);
        assert_eq!(
            crate::sim::find_nearby_cell::find_nearby_passable_cell((3, 2), &query, 0),
            Some((2, 1))
        );
        assert_eq!(
            crate::sim::find_nearby_cell::find_nearby_passable_cell((3, 2), &query, 1),
            Some((3, 1)),
            "live frame modulo chooses the second engine-ordered survivor"
        );
    }

    #[test]
    fn naval_rally_walk_observes_first_building_list_order() {
        let mut occupancy = OccupancyGrid::new();
        let append = crate::sim::occupancy::CellListInsertion::AppendBuilding;
        occupancy.add(12, 12, 10, MovementLayer::Ground, None, append);
        occupancy.add(13, 12, 11, MovementLayer::Ground, None, append);
        occupancy.add(13, 12, 10, MovementLayer::Ground, None, append);
        assert_eq!(
            naval_rally_walk_candidate(10, (10, 10), (12, 12), (20, 10), &occupancy),
            (13, 12),
            "a different first Building stops even when the producer is later"
        );

        let mut producer_first = OccupancyGrid::new();
        producer_first.add(12, 12, 10, MovementLayer::Ground, None, append);
        producer_first.add(13, 12, 10, MovementLayer::Ground, None, append);
        producer_first.add(13, 12, 11, MovementLayer::Ground, None, append);
        assert_eq!(
            naval_rally_walk_candidate(10, (10, 10), (12, 12), (20, 10), &producer_first,),
            (14, 12),
            "producer-first order continues through the foundation run"
        );
    }

    #[test]
    fn naval_rally_fast_path_requires_water_empty_and_playfield_then_bypasses_fnpc() {
        let terrain = water_terrain(32, 32);
        let bounds = test_playfield_bounds();
        let mut occupancy = OccupancyGrid::new();
        let append = crate::sim::occupancy::CellListInsertion::AppendBuilding;
        occupancy.add(12, 12, 10, MovementLayer::Ground, None, append);
        occupancy.add(13, 12, 10, MovementLayer::Ground, None, append);
        let mut entities = EntityStore::new();
        entities.insert(gayard_producer());

        assert_eq!(
            naval_rally_fast_path_cell(
                10,
                (10, 10),
                (12, 12),
                (20, 10),
                &occupancy,
                &entities,
                Some(&terrain),
                Some(bounds),
            ),
            Some((14, 12))
        );

        let mut land_failure = water_terrain(32, 32);
        land_failure.cell_mut(14, 12).unwrap().yr_cell_land_type =
            crate::rules::terrain_rules::LandType::Clear.as_index();
        assert_eq!(
            naval_rally_fast_path_cell(
                10,
                (10, 10),
                (12, 12),
                (20, 10),
                &occupancy,
                &entities,
                Some(&land_failure),
                Some(bounds),
            ),
            None,
            "LandType gate failure must fall back"
        );

        let mut object_occupancy = occupancy.clone();
        object_occupancy.add(
            14,
            12,
            20,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
        entities.insert(test_entity(
            20,
            crate::map::entities::EntityCategory::Unit,
            (14, 12),
        ));
        assert_eq!(
            naval_rally_fast_path_cell(
                10,
                (10, 10),
                (12, 12),
                (20, 10),
                &object_occupancy,
                &entities,
                Some(&terrain),
                Some(bounds),
            ),
            None,
            "selector-zero active object gate failure must fall back"
        );
        assert_eq!(
            naval_rally_fast_path_cell(
                10,
                (10, 10),
                (12, 12),
                (20, 10),
                &occupancy,
                &entities,
                Some(&terrain),
                None,
            ),
            None,
            "missing/failed mode-one playfield authority must fall back"
        );

        let grid = PathGrid::from_resolved_terrain(&land_failure);
        assert_eq!(
            find_naval_unit_delivery_cell(
                10,
                10,
                10,
                Some((20, 10)),
                SpeedType::Float,
                Some(&grid),
                &occupancy,
                &entities,
                Some(&land_failure),
                None,
                None,
                0,
                Some(bounds),
                Some((32, 32)),
            ),
            ((12, 12), false),
            "failed direct gate restarts FNPC at the original GetCoords centre"
        );
    }
}
