//! Native G27 comparisons plus retained-list integration regressions.
//! Full retail passive-caller comparisons live with techno_ai::target_scan.

use super::*;
use crate::map::resolved_terrain::test_flat_cell;
use crate::rules::ini_parser::IniFile;
use crate::sim::intern::test_interner;
use crate::sim::occupancy::CellListInsertion;
use serde::Deserialize;

#[derive(Deserialize)]
struct GateInput {
    name: String,
    mapped_cells: [[u16; 2]; 2],
    coords: [[i32; 3]; 2],
    flags: [u32; 2],
    on_bridge: [u8; 2],
    #[serde(default)]
    dummy_flags: u32,
}

#[derive(Deserialize)]
struct GateRow {
    input: GateInput,
    rejected: bool,
    dummy_coord: [i16; 2],
}

#[derive(Deserialize)]
struct GateCorpus {
    native_sha256: String,
    rows: Vec<GateRow>,
}

#[test]
fn bridge_layer_gate_matches_original_flags_coordinates_and_retained_dummy() {
    let corpus: GateCorpus = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_target_layer.json",
    ))
    .unwrap();
    assert_eq!(
        corpus.native_sha256,
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let mut compared = 0;
    let mut noncanonical_bytes = 0;
    for row in &corpus.rows {
        let input = &row.input;
        // GameEntity owns a bool. Original comparisons of noncanonical bytes
        // remain evidence about native width, not representable Rust states.
        if input.on_bridge.iter().any(|byte| *byte > 1) {
            noncanonical_bytes += 1;
            continue;
        }
        let width = input.mapped_cells.iter().map(|xy| xy[0]).max().unwrap() + 1;
        let height = input.mapped_cells.iter().map(|xy| xy[1]).max().unwrap() + 1;
        let mut cells: Vec<_> = (0..height)
            .flat_map(|y| (0..width).map(move |x| test_flat_cell(x, y)))
            .collect();
        for (xy, flags) in input.mapped_cells.iter().zip(input.flags) {
            cells[usize::from(xy[1]) * usize::from(width) + usize::from(xy[0])]
                .bridge_facts
                .raw_flags = flags;
        }
        let mut terrain = ResolvedTerrainGrid::from_cells(width, height, cells);
        terrain.test_set_native_allocated_cells(&input.mapped_cells.map(|[x, y]| (x, y)));
        let dummy = terrain.shared_cell_dummy();
        dummy.stamp_coord(1234, -2345);
        dummy.write_raw_flags(input.dummy_flags);
        let coord = |index: usize| {
            let [x, y, z] = input.coords[index];
            DriveCoord { x, y, z }
        };
        let before_epoch = terrain.mutation_epoch();
        assert_eq!(
            bridge_layer_rejects_candidate(
                &terrain,
                coord(0),
                input.on_bridge[0] != 0,
                coord(1),
                input.on_bridge[1] != 0,
            ),
            row.rejected,
            "{}",
            input.name
        );
        assert_eq!(
            dummy.snapshot().coord,
            (i32::from(row.dummy_coord[0]), i32::from(row.dummy_coord[1])),
            "{} lookup order",
            input.name
        );
        assert_eq!(dummy.raw_flags(), input.dummy_flags, "{}", input.name);
        assert_eq!(terrain.mutation_epoch(), before_epoch, "{}", input.name);
        for (xy, flags) in input.mapped_cells.iter().zip(input.flags) {
            assert_eq!(
                terrain.cell(xy[0], xy[1]).unwrap().bridge_facts.raw_flags,
                flags,
                "{}",
                input.name
            );
        }
        compared += 1;
    }
    assert_eq!(noncanonical_bytes, 4);
    assert_eq!(compared, corpus.rows.len() - noncanonical_bytes);
    assert_eq!(compared, 155);
}

fn list_fixture() -> (RuleSet, EntityStore, OccupancyGrid, ResolvedTerrainGrid) {
    // Synthetic traversal fixture, not a retail type or native AI admission
    // claim. The native G27 values are compared independently above.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=SCANNER\n1=TARGET\n\
         [SCANNER]\nStrength=200\nPrimary=GUN\nGuardRange=8\n\
         [TARGET]\nStrength=200\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=20\nRange=8\nProjectile=SHOT\nWarhead=WH\n\
         [SHOT]\nAG=yes\nAA=no\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    let mut entities = EntityStore::new();
    let mut occupancy = OccupancyGrid::new();
    for (id, kind, owner, x, on_bridge) in [
        (1, "SCANNER", "Americans", 5, false),
        (2, "TARGET", "Russians", 6, true),
        (3, "TARGET", "Russians", 6, false),
    ] {
        let mut entity = GameEntity::test_default(id, kind, owner, x, 5);
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = true;
        entity.on_bridge = on_bridge;
        occupancy.add(
            x,
            5,
            id,
            if on_bridge {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            },
            None,
            CellListInsertion::PrependNonBuilding,
        );
        entities.insert(entity);
    }
    let cells = (0..20)
        .flat_map(|y| {
            (0..20).map(move |x| {
                let mut cell = test_flat_cell(x, y);
                if y == 5 && (x == 5 || x == 6) {
                    cell.bridge_facts.raw_flags = 0x100;
                }
                cell
            })
        })
        .collect();
    (
        rules,
        entities,
        occupancy,
        ResolvedTerrainGrid::from_cells(20, 20, cells),
    )
}

fn pick(
    rules: &RuleSet,
    entities: &EntityStore,
    occupancy: &OccupancyGrid,
    terrain: &ResolvedTerrainGrid,
    mission: super::super::ScanMission,
) -> Option<u64> {
    super::super::acquire_best_target_for_entity(
        entities,
        occupancy,
        rules,
        &test_interner(),
        1,
        None,
        Some(terrain),
        false,
        mission,
        None,
        super::super::line_of_fire::LineOfFireInputs::default(),
        None,
        None,
    )
    .target()
}

#[test]
fn bridge_layer_rejection_does_not_retry_the_ground_list() {
    let (rules, entities, occupancy, terrain) = list_fixture();
    assert_eq!(
        pick(
            &rules,
            &entities,
            &occupancy,
            &terrain,
            super::super::ScanMission::Guard
        ),
        None,
        "the selected hostile deck head rejects; the ground list is not tried"
    );
    assert_eq!(
        pick(
            &rules,
            &entities,
            &occupancy,
            &terrain,
            super::super::ScanMission::Hunt
        ),
        Some(3),
        "the flat caller reaches each object and can choose the legal ground target"
    );
}

#[test]
fn bridge_layer_gate_reads_current_flags_in_each_scan() {
    let (rules, entities, occupancy, mut terrain) = list_fixture();
    let target_cell = NativeCellQuery::canonical(&terrain).lookup((6, 5));
    for (flags, expected) in [(0x100, None), (0x400, Some(2)), (0x100, None)] {
        // Isolate publication visibility; full destruction/repair callbacks
        // and their object consequences are covered by world integration.
        terrain.write_native_cell_flags(target_cell, flags);
        assert_eq!(
            pick(
                &rules,
                &entities,
                &occupancy,
                &terrain,
                super::super::ScanMission::Guard
            ),
            expected,
            "raw target flags {flags:x}"
        );
    }
}

#[test]
fn original_dead_missing_cell_candidate_runs_fire_probe_before_health_rejection() {
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::retail_ini_fixture::retail_rules_and_art;
    use crate::sim::combat::fire_error::FireError;
    use crate::sim::combat::fire_error_world::FireSubject;
    use crate::sim::movement::{FacingClass, locomotor::LocomotorState};
    use crate::sim::world::Simulation;
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_target_composed.json",
    ))
    .unwrap();
    let row = &native["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["direct_evaluate"] == true)
        .unwrap()["steps"][0];
    let mut world = Simulation::with_seed(31);
    world.intern_rule_type_ids(&rules);
    world.session.binary_frame = 173;
    let obj = rules.object("FV").unwrap();
    for (id, owner, x, y, hp) in [(1, "Americans", 10, 20, 200), (2, "Russians", 41, 41, 0)] {
        let owner = world.intern(owner);
        let type_ref = world.intern("FV");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            id,
            x,
            y,
            6,
            0,
            owner,
            crate::sim::components::Health { current: hp },
            type_ref,
            EntityCategory::Unit,
            0,
            6,
            true,
        );
        entity.position.z = 6;
        entity.position.exact_z_leptons = Some(624);
        entity.health.current = hp;
        entity.lifecycle.cell_marked = true;
        entity.lifecycle.in_limbo = false;
        entity.in_playfield = true;
        entity.locomotor = Some(LocomotorState::from_object_type(obj, 0));
        entity.barrel_facing = Some(FacingClass::new(16383, 0));
        world.substrate.entities.insert(entity);
    }
    world.resolve_type_handles(&rules);
    let mut terrain = ResolvedTerrainGrid::from_cells(
        25,
        25,
        (0..25)
            .flat_map(|y| {
                (0..25).map(move |x| {
                    let mut c = test_flat_cell(x, y);
                    c.level = 6;
                    c
                })
            })
            .collect(),
    );
    terrain.test_set_native_allocated_cells(&[(10, 20)]);
    world.install_resolved_terrain_for_new_map(terrain);
    let terrain = world.resolved_terrain.as_ref().unwrap();
    let dummy = terrain.shared_cell_dummy();
    dummy.stamp_coord(111, -222);
    let firer = world.substrate.entities.get(1).unwrap();
    let candidate = world.substrate.entities.get(2).unwrap();
    let interner = &world.interner;
    let snapshot =
        super::super::build_attacker_snapshot(firer, super::super::TargetKind::Entity(2), None);
    let context = ScanContext {
        entities: &world.substrate.entities,
        los: Default::default(),
        rules: &rules,
        interner: &interner,
        attacker: &snapshot,
        attacker_obj: obj,
        fog: None,
        terrain: Some(terrain),
        require_playfield_membership: true,
        range: ScanRange::CanFireAt,
        coefficients: ThreatCoefficients::resolve(&rules, obj, true),
        zone_grid: None,
        mask: 1,
        scan_coord: [0, 0, 0],
        standing: ScannerStanding::resolve(Some(&world), &snapshot),
        attacks_allies: false,
        scans_allies: false,
        fire_world: Some(&world),
    };
    let walk = WalkArgs {
        flags: 0x8042,
        zone: None,
        reference: ThreatReference::NullCoord,
    };
    let rng_before = world.scenario_rng.native_state_hex();
    assert_eq!(evaluate_candidate(&context, walk, candidate), None);
    let expected = &row["after"]["dummy_coord"];
    assert_eq!(
        dummy.snapshot().coord,
        (
            expected[0].as_i64().unwrap() as i32,
            expected[1].as_i64().unwrap() as i32
        )
    );
    assert_eq!(world.scenario_rng.native_state_hex(), rng_before);
    assert_eq!(row["rng_before_hex"], row["rng_after_hex"]);
    // The native early probe is Facing(2), which Evaluate permits before
    // rejecting health0. Replacing it with a pre-health short circuit loses
    // the shared Dummy lookup observed above.
    assert!(
        row["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["fire_error_return"] == 2)
    );
    assert_eq!(
        FireSubject {
            world: &world,
            rules: &rules,
            overlay_registry: None,
            fog: None,
            firer,
            obj,
            target: Some(super::super::TargetKind::Entity(2)),
            weapon_index: 0,
            garrison: None,
        }
        .fire_error(false),
        FireError::Facing
    );
}
