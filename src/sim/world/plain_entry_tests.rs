//! The plain-step shortcut ([`plain_entry`]) against the full +1AC body
//! ([`full_foot_entry`]): it answers only steps whose every test before the
//! land row passes, and then as the body does.

use super::*;
use crate::map::cell_index::NativeCellIdentity;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::occupancy::CellListInsertion;
use crate::sim::world::entry_test_fixture;

/// Beside the fixture's Foot engineer: a Wheel truck, a Unit restricted to
/// Clear land, a Unit whose SpeedType does not parse, the stone wall `O2`
/// and the crate `O4`.
const RULES: &str = "[VehicleTypes]\n0=TRUCK\n1=BOAT\n2=ODD\n\
     [TRUCK]\nSpeedType=Wheel\nStrength=100\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [BOAT]\nSpeedType=Wheel\nMovementRestrictedTo=Clear\nStrength=100\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [ODD]\nSpeedType=Bogus\nStrength=100\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [O2]\nWall=yes\nStrength=300\n[O4]\nCrate=yes\n";

const ENGINEER: u64 = 90;
const TRUCK: u64 = 91;
const BOAT: u64 = 92;
const ODD: u64 = 93;

/// The step east from (15, 15) to (16, 15), direction 2.
const SOURCE: (i16, i16) = (15, 15);
const TARGET: (i16, i16) = (16, 15);
const EAST: i32 = 2;

fn fixture() -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
) {
    let (mut sim, rules, registry) = entry_test_fixture::fixture_with_rules(RULES);
    let owner = sim.intern("Americans");
    for (id, name, category, kind) in [
        (
            ENGINEER,
            "ENGINEER",
            EntityCategory::Infantry,
            LocomotorKind::Walk,
        ),
        (TRUCK, "TRUCK", EntityCategory::Unit, LocomotorKind::Drive),
        (BOAT, "BOAT", EntityCategory::Unit, LocomotorKind::Drive),
        (ODD, "ODD", EntityCategory::Unit, LocomotorKind::Drive),
    ] {
        let mut mover = GameEntity::test_default(id, name, "Americans", 15, 15);
        mover.set_owner_for_test(owner);
        mover.set_type_ref_for_test(sim.intern(name));
        mover.set_category_for_test(category);
        mover.set_in_playfield(true);
        mover.locomotor = Some(LocomotorState::for_test_kind(kind));
        sim.substrate.entities.insert(mover);
    }
    (sim, rules, registry)
}

fn cell(sim: &Simulation, coord: (i16, i16)) -> NativeCellIdentity {
    sim.resolved_terrain
        .as_ref()
        .unwrap()
        .native_cell_identity(coord)
}

fn terrain_cell(
    sim: &mut Simulation,
    (x, y): (i16, i16),
) -> &mut crate::map::resolved_terrain::ResolvedTerrainCell {
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(x as u16, y as u16)
        .unwrap()
}

/// A tube at `at`, rebuilding the terrain with it.
fn tube_at(sim: &mut Simulation, at: (i16, i16)) {
    use crate::map::tube_facts::{TubeFact, TubeId, TubeSource};
    let mut cells = sim.resolved_terrain.take().unwrap().cells().to_vec();
    let at = (at.0 as u16, at.1 as u16);
    cells
        .iter_mut()
        .find(|cell| (cell.rx, cell.ry) == at)
        .unwrap()
        .tube_index = Some(TubeId(0));
    let tube = TubeFact {
        entry: at,
        exit: (20, 20),
        direction: EAST,
        path_steps: Vec::new(),
        source: TubeSource::ExplicitMap,
    };
    sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells_with_tubes(
        33,
        33,
        cells,
        vec![tube],
    ));
}

/// Mover `id`'s step to `target`: the shortcut's answer through a path
/// batch's facts, and the full body's from the world, each reading
/// `registry`.
fn step_to(
    sim: &Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    id: u64,
    target: (i16, i16),
    args: InfantryEntryArgs,
) -> (Option<u8>, Result<u8, String>) {
    let kept = EntryFacts::fitted(sim);
    let batched = sim
        .foot_entry_receiver(id, rules, registry, Some(&kept))
        .unwrap();
    let world = sim.foot_entry_receiver(id, rules, registry, None).unwrap();
    let target = cell(sim, target);
    (
        plain_entry(&batched.live, &batched.mover, target, args),
        full_foot_entry(&world.live, &world.mover, target, args),
    )
}

/// Mover `id`'s step to `TARGET`.
fn step(
    sim: &Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    id: u64,
    args: InfantryEntryArgs,
) -> (Option<u8>, Result<u8, String>) {
    step_to(sim, rules, Some(registry), id, TARGET, args)
}

fn east(sim: &Simulation, height: i32) -> InfantryEntryArgs {
    InfantryEntryArgs {
        direction: EAST,
        height,
        previous_cell: Some(cell(sim, SOURCE)),
    }
}

/// Open ground, a plain overlay, a zero land row (which a tethered Infantry
/// does not read) and a level change the path height already made are
/// answered, each as the full body answers.
#[test]
fn a_plain_step_answers_as_the_full_body() {
    let (mut sim, rules, registry) = fixture();
    let answered = |sim: &Simulation, id, height| {
        let (plain, full) = step(sim, &rules, &registry, id, east(sim, height));
        assert_eq!(plain.map(Ok), Some(full.clone()), "mover {id}");
        full.unwrap()
    };
    for id in [ENGINEER, TRUCK] {
        assert_eq!(answered(&sim, id, 0), 0);
    }
    // An overlay that is neither wall nor crate (O5 is plain Clear land).
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(16, 15, 5, 0);
    for id in [ENGINEER, TRUCK] {
        assert_eq!(answered(&sim, id, 0), 0);
    }
    // A zero Wheel row refuses only the truck.
    terrain_cell(&mut sim, TARGET).speed_costs.wheel = Some(0);
    assert_eq!(answered(&sim, ENGINEER, 0), 0);
    assert_eq!(answered(&sim, TRUCK, 0), 7);
    terrain_cell(&mut sim, TARGET).speed_costs.foot = Some(0);
    assert_eq!(answered(&sim, ENGINEER, 0), 7);
    sim.substrate
        .entities
        .get_mut(ENGINEER)
        .unwrap()
        .tether
        .set_for_test(Some(TRUCK));
    assert_eq!(answered(&sim, ENGINEER, 0), 0);
    // A higher target the path height has reached: the prelude compares the
    // path height, not the source's level, with a flat source's target.
    terrain_cell(&mut sim, TARGET).level = 1;
    assert_eq!(answered(&sim, ENGINEER, 1), 0);
}

/// Every step the body might answer otherwise, or might not answer at all,
/// goes to the body.
#[test]
fn a_step_needing_more_than_the_land_row_takes_the_full_body() {
    let skipped = |change: &dyn Fn(&mut Simulation),
                   args: &dyn Fn(&Simulation) -> InfantryEntryArgs| {
        let (mut sim, rules, registry) = fixture();
        change(&mut sim);
        for id in [ENGINEER, TRUCK] {
            assert_eq!(step(&sim, &rules, &registry, id, args(&sim)).0, None);
        }
    };
    let level = |sim: &Simulation| east(sim, 0);
    // Raw occupation, a listed object and a wall on the target.
    skipped(
        &|sim| sim.substrate.raw_cell_occupation.mark_ground(16, 15, 0x20),
        &level,
    );
    skipped(
        &|sim| {
            let mut blocker = GameEntity::test_default(92, "TRUCK", "Russians", 16, 15);
            blocker.set_owner_for_test(sim.intern("Russians"));
            blocker.set_type_ref_for_test(sim.intern("TRUCK"));
            blocker.set_category_for_test(EntityCategory::Unit);
            sim.substrate.entities.insert(blocker);
            sim.substrate.occupancy.add(
                16,
                15,
                92,
                MovementLayer::Ground,
                None,
                CellListInsertion::from_category(EntityCategory::Unit),
            );
        },
        &level,
    );
    skipped(
        &|sim| {
            sim.overlay_grid
                .as_mut()
                .unwrap()
                .place_overlay(16, 15, 2, 0);
        },
        &level,
    );
    // A crate, and a plain overlay whose type the asker cannot read.
    skipped(
        &|sim| {
            sim.overlay_grid
                .as_mut()
                .unwrap()
                .place_overlay(16, 15, 4, 0);
        },
        &level,
    );
    {
        let (mut sim, rules, _) = fixture();
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(16, 15, 5, 0);
        for id in [ENGINEER, TRUCK] {
            let args = east(&sim, 0);
            assert_eq!(step_to(&sim, &rules, None, id, TARGET, args).0, None);
        }
    }
    // A tube on either cell.
    skipped(&|sim| tube_at(sim, TARGET), &level);
    skipped(&|sim| tube_at(sim, SOURCE), &level);
    // A structural bridge on either cell, and an unresolved land row.
    skipped(
        &|sim| terrain_cell(sim, TARGET).bridge_facts.raw_flags = 0x100,
        &level,
    );
    skipped(
        &|sim| terrain_cell(sim, SOURCE).bridge_facts.raw_flags = 0x100,
        &level,
    );
    skipped(
        &|sim| {
            let costs = &mut terrain_cell(sim, TARGET).speed_costs;
            (costs.foot, costs.wheel) = (None, None);
        },
        &level,
    );
    // A level change, a previous cell other than the backstep, the tube
    // jump, an undirected ask and an unknown height.
    skipped(&|sim| terrain_cell(sim, TARGET).level = 1, &level);
    skipped(&|_| {}, &|sim| InfantryEntryArgs {
        previous_cell: Some(cell(sim, (15, 14))),
        ..east(sim, 0)
    });
    for direction in [8, -1] {
        skipped(&|_| {}, &|sim| InfantryEntryArgs {
            direction,
            ..east(sim, 0)
        });
    }
    skipped(&|_| {}, &|sim| east(sim, -1));
}

/// A target outside the playfield, a Unit restricted to a land type, and a
/// Unit with no land row (an unparsed SpeedType) go to the body.
#[test]
fn a_boundary_a_restricted_unit_or_an_unknown_speed_takes_the_full_body() {
    let (sim, rules, registry) = fixture();
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let query = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    let outside = (0..33)
        .flat_map(|y| (1..33).map(move |x| (x, y)))
        .find(|&target| {
            !crate::sim::cell_rect::retained_cell_is_in_playfield(
                cell(&sim, target),
                sim.playfield_bounds,
                &query,
            )
        })
        .expect("a cell outside the playfield");
    let from = (outside.0 - 1, outside.1);
    let args = InfantryEntryArgs {
        previous_cell: Some(cell(&sim, from)),
        ..east(&sim, 0)
    };
    for id in [ENGINEER, TRUCK] {
        assert_eq!(
            step_to(&sim, &rules, Some(&registry), id, outside, args).0,
            None
        );
    }
    assert_eq!(step(&sim, &rules, &registry, BOAT, east(&sim, 0)).0, None);
    let (plain, full) = step(&sim, &rules, &registry, ODD, east(&sim, 0));
    assert_eq!(plain, None);
    assert!(full.is_err(), "{full:?}");
}
