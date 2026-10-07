//! Native ObjectGetCell comparisons and FireError's retained Cell identities.

use super::{bridge_scene, rules};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid, test_flat_cell};
use crate::sim::combat::TargetKind;
use crate::sim::combat::fire_error::{CellFacts, FireQuery, FirerCell};
use crate::sim::combat::fire_error_world::{FireSubject, WorldQuery};
use crate::sim::components::DriveCoord;
use crate::sim::movement::ground_pose::query_object_cell;
use crate::util::fixed_math::SimFixed;
use serde::Deserialize;

#[derive(Deserialize)]
struct CellInput {
    xy: [i16; 2],
    land: i32,
    flags: u32,
}

#[derive(Deserialize)]
struct Input {
    name: String,
    xyz: [i32; 3],
    marked: u8,
    mapped_cells: Vec<CellInput>,
    dummy: CellInput,
}

#[derive(Deserialize)]
struct Row {
    input: Input,
    identity: String,
    land: i32,
    flags: u32,
    dummy_xy: [i16; 2],
}

#[derive(Deserialize)]
struct Corpus {
    native_sha256: String,
    rows: Vec<Row>,
}

fn terrain(input: &Input) -> ResolvedTerrainGrid {
    let width = input
        .mapped_cells
        .iter()
        .map(|cell| cell.xy[0] as u16)
        .max()
        .unwrap()
        + 1;
    let height = input
        .mapped_cells
        .iter()
        .map(|cell| cell.xy[1] as u16)
        .max()
        .unwrap()
        + 1;
    let mut cells: Vec<_> = (0..height)
        .flat_map(|y| (0..width).map(move |x| test_flat_cell(x, y)))
        .collect();
    for cell in &input.mapped_cells {
        let index = cell.xy[1] as usize * usize::from(width) + cell.xy[0] as usize;
        cells[index].yr_cell_land_type = cell.land as u8;
        cells[index].bridge_facts.raw_flags = cell.flags;
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(width, height, cells);
    terrain.test_set_native_allocated_cells(
        &input
            .mapped_cells
            .iter()
            .map(|cell| (cell.xy[0] as u16, cell.xy[1] as u16))
            .collect::<Vec<_>>(),
    );
    reset_dummy(&terrain, &input.dummy);
    terrain
}

fn reset_dummy(terrain: &ResolvedTerrainGrid, input: &CellInput) {
    let dummy = terrain.shared_cell_dummy();
    dummy.stamp_coord(i32::from(input.xy[0]), i32::from(input.xy[1]));
    dummy.test_set_land_type(input.land);
    dummy.write_raw_flags(input.flags);
}

#[test]
fn original_object_cell_queries_keep_physical_aliases_and_dummy_fields() {
    let corpus: Corpus = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/object_get_cell.json",
    ))
    .unwrap();
    assert_eq!(
        corpus.native_sha256,
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    assert_eq!(corpus.rows.len(), 10);
    for row in corpus.rows {
        let terrain = terrain(&row.input);
        let cells = NativeCellQuery::canonical(&terrain);
        let [x, y, z] = row.input.xyz;
        let cell = query_object_cell(&cells, DriveCoord { x, y, z });
        let expected_identity = match row.identity.as_str() {
            "dummy" => NativeCellIdentity::Dummy,
            "cell_a" | "cell_b" => {
                let index = usize::from(row.identity == "cell_b");
                let [x, y] = row.input.mapped_cells[index].xy;
                NativeCellIdentity::Real(terrain.native_fixed_cell_index(x, y).unwrap())
            }
            other => panic!("unexpected original identity {other}"),
        };
        assert_eq!(cell, expected_identity, "{}", row.input.name);
        assert_eq!(cells.land_type(cell), row.land, "{}", row.input.name);
        assert_eq!(cells.flags(cell), row.flags, "{}", row.input.name);
        assert_eq!(
            terrain.shared_cell_dummy().snapshot().coord,
            (i32::from(row.dummy_xy[0]), i32::from(row.dummy_xy[1])),
            "{}",
            row.input.name
        );

        // Position can express the ordinary/negative-fraction/alias/missing
        // controls. The full i32 extremes above belong to the map-query seam.
        if x == i32::MIN || y == i32::MAX {
            continue;
        }
        let rules = rules();
        let mut sim = bridge_scene("TANK");
        sim.resolved_terrain = Some(terrain);
        for id in [1, 2] {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.position.rx = (x.max(0) / 256) as u16;
            entity.position.ry = (y.max(0) / 256) as u16;
            entity.position.sub_x = SimFixed::from_num(x - i32::from(entity.position.rx) * 256);
            entity.position.sub_y = SimFixed::from_num(y - i32::from(entity.position.ry) * 256);
            entity.position.exact_z_leptons = Some(z);
            entity.lifecycle.cell_marked = row.input.marked != 0;
        }
        let subject = FireSubject {
            world: &sim,
            rules: &rules,
            overlay_registry: None,
            fog: None,
            firer: sim.substrate.entities.get(1).unwrap(),
            obj: rules.object("TANK").unwrap(),
            target: Some(TargetKind::Entity(2)),
            weapon_index: 0,
            garrison: None,
        };
        let mut query = WorldQuery::new(&subject);
        let expected = CellFacts {
            land_type: row.land,
            flags: row.flags,
        };
        reset_dummy(sim.resolved_terrain.as_ref().unwrap(), &row.input.dummy);
        assert_eq!(query.target_cell(), Some(expected), "{}", row.input.name);
        assert_eq!(
            query.firer_cell(),
            FirerCell::Cell(expected),
            "{}",
            row.input.name
        );
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .shared_cell_dummy()
                .snapshot()
                .coord,
            (i32::from(row.dummy_xy[0]), i32::from(row.dummy_xy[1])),
            "{}",
            row.input.name
        );
        assert_eq!(subject.firer.lifecycle.cell_marked, row.input.marked != 0);
    }
}

/// Pointer comparisons use allocation identity, after Object5F6960's reads.
/// These integration controls reuse the original alias/miss inputs above;
/// they do not claim a whole Infantry AreaFire/Cell-target native transcript.
#[test]
fn cell_target_equality_uses_real_aliases_and_one_shared_dummy() {
    let rules = rules();
    let mut sim = bridge_scene("TANK");
    let mut cells = vec![test_flat_cell(0, 0); 512];
    for (x, cell) in cells.iter_mut().enumerate() {
        *cell = test_flat_cell(x as u16, 0);
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(512, 1, cells);
    terrain.test_set_native_allocated_cells(&[(511, 0)]);
    sim.resolved_terrain = Some(terrain);
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.position.rx = 511;
    entity.position.ry = 0;
    entity.position.sub_x = SimFixed::from_num(128);
    entity.position.sub_y = SimFixed::from_num(128);
    let subject = FireSubject {
        world: &sim,
        rules: &rules,
        overlay_registry: None,
        fog: None,
        firer: sim.substrate.entities.get(1).unwrap(),
        obj: rules.object("TANK").unwrap(),
        target: Some(TargetKind::Cell(u16::MAX, 1)),
        weapon_index: 0,
        garrison: None,
    };
    assert_eq!(WorldQuery::new(&subject).firer_cell(), FirerCell::Target);

    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.position.rx = 41;
    entity.position.ry = 41;
    let dummy = sim.resolved_terrain.as_ref().unwrap().shared_cell_dummy();
    dummy.stamp_coord(17, -19);
    dummy.test_set_land_type(6);
    let subject = FireSubject {
        world: &sim,
        rules: &rules,
        overlay_registry: None,
        fog: None,
        firer: sim.substrate.entities.get(1).unwrap(),
        obj: rules.object("TANK").unwrap(),
        target: Some(TargetKind::Cell(42, 42)),
        weapon_index: 0,
        garrison: None,
    };
    assert_eq!(subject.facts().target.land_type, 6);
    assert_eq!(
        dummy.snapshot().coord,
        (17, -19),
        "facts do not resolve again"
    );
    let mut query = WorldQuery::new(&subject);
    query.retain_target_center_cell();
    assert_ne!(
        dummy.snapshot().coord,
        (42, 42),
        "read the retained receiver, not the requested coordinate"
    );
    assert_eq!(query.target_center_cell, Some(NativeCellIdentity::Dummy));
    assert_eq!(query.firer_cell(), FirerCell::Target);
    assert_eq!(dummy.snapshot().coord, (41, 41));
}

/// Instructions at 0x006FC19C retain EDI until 0x004870D0; nested misses must
/// not turn that pointer into an unrelated real-cell sensor query.
#[test]
fn sensor_reads_retained_identity_after_another_query_stamps_dummy() {
    let rules = rules();
    let mut sim = bridge_scene("TANK");
    let owner = sim.substrate.entities.get(1).unwrap().owner();
    sim.fog.width = 16;
    sim.fog.height = 16;
    sim.fog.increment_sensor_at(owner, 12, 10);
    let subject = FireSubject {
        world: &sim,
        rules: &rules,
        overlay_registry: None,
        fog: Some(&sim.fog),
        firer: sim.substrate.entities.get(1).unwrap(),
        obj: rules.object("TANK").unwrap(),
        target: Some(TargetKind::Entity(2)),
        weapon_index: 0,
        garrison: None,
    };
    let mut query = WorldQuery::new(&subject);
    query.retain_target_center_cell();
    let dummy = sim.resolved_terrain.as_ref().unwrap().shared_cell_dummy();
    dummy.stamp_coord(41, 41);
    assert!(query.sensor());
    query.target_center_cell = Some(NativeCellIdentity::Dummy);
    dummy.stamp_coord(12, 10);
    assert!(
        !query.sensor(),
        "the dummy has no modeled sensor-counter owner"
    );
}

#[derive(Deserialize)]
struct EarlyInput {
    initial_dummy_xy: [i16; 2],
    target_xyz: [i32; 3],
}

#[derive(Deserialize)]
struct EarlyRow {
    name: String,
    supplied: EarlyInput,
    result: i32,
    retained_center: Vec<String>,
    dummy_xy: [i16; 2],
}

#[derive(Deserialize)]
struct CenterRow {
    name: String,
    input_xy: [i16; 2],
    receiver: String,
    identity: String,
    dummy_xy: [i16; 2],
}

#[derive(Deserialize)]
struct BoundaryCorpus {
    native_sha256: String,
    rows: Vec<EarlyRow>,
    cell_centers: Vec<CenterRow>,
}

fn boundary_corpus() -> BoundaryCorpus {
    let corpus: BoundaryCorpus = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fire_error_cell_boundary.json",
    ))
    .unwrap();
    assert_eq!(
        corpus.native_sha256,
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    corpus
}

/// The native companion supplies the same special lifecycle fields. This
/// compares the base ladder's early exits and map side effects, not Temporal
/// or Magnetron production. The admitted Unit prefix adds no map query.
#[test]
fn original_early_fire_error_retains_cell_between_temporal_and_lifted_gates() {
    use crate::sim::combat::fire_error::{Link, get_fire_error};
    let corpus = boundary_corpus();
    assert_eq!(corpus.rows.len(), 4);
    for row in corpus.rows {
        let rules = rules();
        let mut sim = bridge_scene("TANK");
        let target = sim.substrate.entities.get_mut(2).unwrap();
        let [x, y, z] = row.supplied.target_xyz;
        target.position.rx = (x / 256) as u16;
        target.position.ry = (y / 256) as u16;
        target.position.sub_x = SimFixed::from_num(x % 256);
        target.position.sub_y = SimFixed::from_num(y % 256);
        target.position.exact_z_leptons = Some(z);
        if row.name == "target_cloaked" {
            let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0, 1);
            cloak.state = 2;
            target.cloak = Some(cloak);
        }
        let dummy = sim.resolved_terrain.as_ref().unwrap().shared_cell_dummy();
        let [x, y] = row.supplied.initial_dummy_xy;
        dummy.stamp_coord(i32::from(x), i32::from(y));
        let subject = FireSubject {
            world: &sim,
            rules: &rules,
            overlay_registry: None,
            fog: Some(&sim.fog),
            firer: sim.substrate.entities.get(1).unwrap(),
            obj: rules.object("TANK").unwrap(),
            target: Some(TargetKind::Entity(2)),
            weapon_index: 0,
            garrison: None,
        };
        let mut facts = subject.facts();
        match row.name.as_str() {
            "temporal_held_target" => facts.firer.temporal = Some(Link::Target),
            "firer_lifted" => facts.firer.magnetron_lifted = true,
            "target_limbo" => facts.target.in_limbo = true,
            "target_cloaked" => {}
            other => panic!("unexpected native early control {other}"),
        }
        let mut query = WorldQuery::new(&subject);
        assert_eq!(
            get_fire_error(&facts, &mut query, false) as i32,
            row.result,
            "{}",
            row.name
        );
        assert_eq!(
            query.target_center_cell,
            (!row.retained_center.is_empty()).then_some(NativeCellIdentity::Dummy),
            "{}",
            row.name
        );
        assert_eq!(
            dummy.snapshot().coord,
            (i32::from(row.dummy_xy[0]), i32::from(row.dummy_xy[1])),
            "{}",
            row.name
        );
    }
}

#[test]
fn original_cell_center_query_reads_the_retained_receiver_coordinate() {
    let corpus = boundary_corpus();
    assert_eq!(corpus.cell_centers.len(), 3);
    for row in corpus.cell_centers {
        let rules = rules();
        let mut sim = bridge_scene("TANK");
        let mut cells: Vec<_> = (0..32)
            .flat_map(|y| (0..32).map(move |x| test_flat_cell(x, y)))
            .collect();
        cells[20 * 32 + 10].level = 6;
        let mut terrain = ResolvedTerrainGrid::from_cells(32, 32, cells);
        terrain.test_set_native_allocated_cells(&[(10, 20)]);
        sim.resolved_terrain = Some(terrain);
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let dummy = terrain.shared_cell_dummy();
        dummy.stamp_coord(111, -222);
        let target = if row.receiver == "dummy" {
            dummy.stamp_coord(i32::from(row.input_xy[0]), i32::from(row.input_xy[1]));
            TargetKind::Cell(42, 42)
        } else {
            TargetKind::Cell(row.input_xy[0] as u16, row.input_xy[1] as u16)
        };
        let subject = FireSubject {
            world: &sim,
            rules: &rules,
            overlay_registry: None,
            fog: None,
            firer: sim.substrate.entities.get(1).unwrap(),
            obj: rules.object("TANK").unwrap(),
            target: Some(target),
            weapon_index: 0,
            garrison: None,
        };
        let mut query = WorldQuery::new(&subject);
        query.retain_target_center_cell();
        let identity = if row.identity == "dummy" {
            NativeCellIdentity::Dummy
        } else {
            NativeCellIdentity::Real(terrain.native_fixed_cell_index(10, 20).unwrap())
        };
        assert_eq!(query.target_center_cell, Some(identity), "{}", row.name);
        assert_eq!(
            dummy.snapshot().coord,
            (i32::from(row.dummy_xy[0]), i32::from(row.dummy_xy[1])),
            "{}",
            row.name
        );
    }
}
