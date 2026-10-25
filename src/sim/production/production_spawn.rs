//! Where `BuildingClass::ExitObject_Main @ 0x00443C60` releases a produced
//! Unit or Infantry: the arm its producer's type picks, and the Cell or
//! coordinate each arm places it at. `production_queue::exit_produced_object`
//! runs the arms.

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::{FactoryType, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
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
/// GetDockCellForObject (vt+0x4D4, `0x0044EFB0`) for `mover` and `offered`,
/// then [`dock_exit_coordinate_and_facing`] for the selected Cell. The
/// Infantry arm's call (`0x004449AF`) offers the empty Cell (`0x0089C818`,
/// zeroed by `0x0043B0E0`); the generic arm's (`0x0044464D`) offers
/// ExitObject's own Cell argument: zero from Factory_AI `0x00450155`,
/// Place_Production `0x004FB563` and Cloning Vat `0x00444A3E`, but the
/// requested origin from reinforcement `0x0065DAB6`.
/// `None` is the empty dock Cell, on which both arms answer 0
/// (`0x00445696`).
pub(super) fn dock_exit(
    sim: &Simulation,
    rules: &RuleSet,
    producer_id: u64,
    mover: u64,
    offered: (i16, i16),
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<DockExit> {
    let cell = crate::sim::docking::building_dock::building_dock_cell(
        sim,
        producer_id,
        mover,
        offered,
        rules,
        overlay_registry,
    )?;
    let producer = sim.substrate.entities.get(producer_id)?;
    let producer_type = sim.entity_object_type(producer, rules)?;
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
    let (width, height) = producer.type_facts.foundation().dimensions();
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
            rules,
            effects,
        );
    }
}

impl Simulation {
    /// Whether `id` is a Building (vt+0x2C == 6) whose type has
    /// `WeaponsFactory=` (BuildingType `+0x16BD`): the war factory test on a
    /// radio contact. The Unit setter's WeaponsFactory arm,
    /// Queue_Navigation_List's idle tail (`0x004DA15A..0x004DA1A6`), Unit
    /// Per_Cell_Process's factory clearance, CloakingTick's destination
    /// exclusion, `UnitClass::Is_Ready_To_Commence`'s contact read
    /// (`0x0074433C`) and [`Self::unit_in_contact_war_factory`] ask it.
    pub(crate) fn is_weapons_factory(&self, id: u64, rules: &RuleSet) -> bool {
        self.substrate.entities.get(id).is_some_and(|building| {
            building.category() == crate::map::entities::EntityCategory::Structure
                && self
                    .entity_object_type(building, rules)
                    .is_some_and(|object| object.weapons_factory)
        })
    }

    /// A Unit whose radio contact slot 0 (`0x0065AD30(0)`: the slot itself,
    /// not the first filled one) is a war factory
    /// ([`Self::is_weapons_factory`]) that is also the first ground building
    /// in the Unit's own cell (`0x0047C520`): one still leaving the factory
    /// that linked it ([`mark_war_factory_spawn_contact`]). Native inlines
    /// the test in `UnitClass::ReceiveDamage` (`0x00737CB6..0x00737D2E`),
    /// `TemporalClass::CanWarpTarget` (`0x0071AE50`), the Chrono Warp
    /// (`SuperClass::Launch 0x006CC7BC..0x006CC858`) and the locomotor imbue
    /// (`0x0046953E..0x004695E6`).
    pub(crate) fn unit_in_contact_war_factory(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(unit) = self.substrate.entities.get(id) else {
            return false;
        };
        unit.category() == crate::map::entities::EntityCategory::Unit
            && unit.radio_contacts.slot(0).is_some_and(|contact| {
                self.is_weapons_factory(contact, rules)
                    && self.substrate.occupancy.first_building_on_layer(
                        unit.position.rx(),
                        unit.position.ry(),
                        MovementLayer::Ground,
                    ) == Some(contact)
            })
    }
}

/// The naval arm's Cell (`0x00444175..0x004443DC`) for a product of
/// `speed_type`.
///
/// `BuildingClass::ExitObject_Main @ 0x00443C60` begins at the producer's
/// `GetCoords` foundation centre, optionally walks out of the producer toward
/// its own ArchiveTarget/rally cell, and otherwise invokes the one shared FNPC
/// call at `0x004443DC`. FNPC failure is the literal zero cell; the caller still
/// performs one normal Unlimbo attempt against that result.
pub(super) fn naval_exit_cell(
    sim: &Simulation,
    producer_id: u64,
    speed_type: SpeedType,
) -> Option<(u16, u16)> {
    let producer = sim.substrate.entities.get(producer_id)?;
    let Some(origin) = producer_get_coords_cell(&sim.substrate.entities, producer_id) else {
        return Some((0, 0));
    };
    if let Some(rally) = producer.rally_cell()
        && let Some(candidate) = naval_rally_fast_path_cell(
            producer_id,
            (producer.position.rx(), producer.position.ry()),
            origin,
            rally,
            &sim.substrate.occupancy,
            &sim.substrate.entities,
            sim.resolved_terrain.as_ref(),
            sim.playfield_bounds(),
        )
    {
        return Some(candidate);
    }
    // 4443DC -> FNPC56DC20 -> rectangle56E7C0 -> Cell4834A0's Float arm reads
    // the selected raw plane with both ignore flags false (56E854..858/
    // 483550..570), so a marked yard footprint is no candidate and the later
    // Unlimbo is not refused. Original control: factory_naval_output; shared
    // raw goldens: tools/spatial_oracle/nearby_raw_occupation.{py,json,meta.json}.
    // The caller passes a pointer to CellStruct(0,0): no target, the
    // live-frame pick, no RNG draw; final object-list occupancy is off.
    let cell = sim.nearby_passable_cell(
        (i32::from(origin.0), i32::from(origin.1)),
        NearbyArgs::plain(
            PassabilityArgs {
                // Unit vtable +0x84 -> +0x88 -> UnitType+0x67C.
                speed_type,
                required_zone_id: None,
                // Caller argument5 is literal0 at 4443B4 (MovementZone::Normal).
                movement_zone: MovementZone::Normal,
                bridge_aware_zone: false,
            },
            NearbyFootprint::SINGLE,
        ),
    );
    Some(cell.unwrap_or((0, 0)))
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
    if terrain.cell(candidate.0, candidate.1)?.land_type
        != crate::rules::terrain_rules::LandType::Water.as_index()
    {
        return None;
    }
    // `CellClass::Find_Nearest_Object @ 0x0047C3D0` (`0x00444329`) takes
    // any Techno in the cell, with no alive or Health test: an infantry
    // corpse refuses the candidate too. `is_active` still skips a vehicle or
    // aircraft on VERA's SHP death animation (the dying mark), a VERA-only
    // chain native does not have.
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
    let facing = crate::util::direction_tables::facing8_from_delta(
        i32::from(rally.0) - i32::from(producer_nw.0),
        i32::from(rally.1) - i32::from(producer_nw.1),
    );
    let (step_x, step_y) = crate::util::direction::delta_from_facing(facing);
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
    use crate::map::resolved_terrain::ResolvedTerrainGrid;

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
        for cell in terrain.cells_mut_for_test() {
            // Retail ground rows used by the compact admission matrix.
            cell.speed_costs.float = Some(0);
            cell.base_speed_costs.float = Some(0);
            cell.speed_costs.hover = Some(50);
            cell.base_speed_costs.hover = Some(50);
        }
        sim.resolved_terrain = Some(terrain);
        sim.set_playfield_bounds_for_test(Some(test_playfield_bounds()));
        sim
    }

    fn set_admission_water_cell(sim: &mut Simulation, cell: (u16, u16)) {
        let water = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(cell.0, cell.1)
            .unwrap();
        water.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
        water.speed_costs.float = Some(100);
        water.base_speed_costs.float = Some(100);
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
            .cell_mut_for_test(8, 8)
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
}
