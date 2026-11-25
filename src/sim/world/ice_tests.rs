//! Firestorm ice on a synthetic snow grid (docs/ts-ice.md).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::terrain_rules::LandType;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::occupancy::CellListInsertion;

/// Ice1/2/3Set, WaterSet and ShorePieces, apart so no window overlaps.
const TILES: IceTiles = IceTiles {
    ice: [100, 200, 300],
    water: 20,
    shore: 400,
};

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\nIceCrackingWeight=2\nIceBreakingWeight=4\n\
         [AudioVisual]\nIceGrowthRate=1\nIceSolidifyFrameTime=500\n\
         [VehicleTypes]\n0=LIGHT\n1=HEAVY\n\
         [LIGHT]\nStrength=100\nWeight=2\n[HEAVY]\nStrength=100\nWeight=4\n",
    ))
    .unwrap()
}

/// A 12x12 grid of Ice1's solid tile, running ice when `ice` is set.
fn snow(ice: bool) -> Simulation {
    let mut sim = Simulation::new();
    super::super::lifecycle_tests::install_common_raw_terrain(&mut sim, 12, 12, 0, None);
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    if ice {
        terrain.test_install_ice_catalog(TILES);
    }
    for y in 0..12 {
        for x in 0..12 {
            terrain.set_ice_tile((x, y), TILES.ice[0], true);
            terrain.recalc_ice_cell((x, y));
        }
    }
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, false, 0, 10));
    sim.session.set_ice_growth(1);
    sim.session.set_binary_frame_for_test(10);
    sim
}

/// A unit of `kind` facing north on the ground of `cell`.
fn place(sim: &mut Simulation, id: u64, kind: &str, cell: (u16, u16)) {
    let owner = sim.interner.intern("Americans");
    let mut entity = GameEntity::test_default(id, kind, "Americans", cell.0, cell.1);
    entity.set_type_ref_for_test(sim.interner.intern(kind));
    entity.set_owner_for_test(owner);
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.lifecycle.set_object_alive_for_test(true);
    sim.substrate.entities.insert(entity);
    sim.substrate.occupancy.add(
        cell.0,
        cell.1,
        id,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );
}

fn tile(sim: &Simulation, cell: (u16, u16)) -> (i32, u8) {
    let cell = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cell(cell.0, cell.1)
        .unwrap();
    (cell.final_tile_index, cell.land_type)
}

fn arrive(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    sim.unit_arrival_ice(id, rules, None, &mut FrameEffects::default());
}

#[test]
fn heavy_units_crack_then_break_the_ice_and_what_stood_there_sinks() {
    let rules = rules();
    // A snow map without the ice sets: no draw, no tile write, no list.
    let mut sim = snow(false);
    place(&mut sim, 1, "HEAVY", (5, 5));
    let rng = sim.scenario_rng.native_state_hex();
    arrive(&mut sim, &rules, 1);
    assert_eq!(sim.scenario_rng.native_state_hex(), rng);
    assert_eq!(tile(&sim, (5, 5)).0, TILES.ice[0]);
    assert!(sim.ice_cracks.is_empty());
    assert!(!sim.substrate.entities.get(1).unwrap().sinking.is_active());

    let mut sim = snow(true);
    place(&mut sim, 1, "LIGHT", (5, 5));
    arrive(&mut sim, &rules, 1);
    assert!(TILES.is_cracked(tile(&sim, (5, 5)).0));
    assert_eq!(
        *sim.ice_cracks,
        [CrackedIce {
            cell: (5, 5),
            frame: 510
        }]
    );
    assert!(!sim.substrate.entities.get(1).unwrap().sinking.is_active());

    // Facing north breaks the 2x2 footprint stepping north-east.
    place(&mut sim, 2, "HEAVY", (5, 5));
    let before = position_world_coord(&sim.substrate.entities.get(2).unwrap().position);
    arrive(&mut sim, &rules, 2);
    for cell in [(5, 5), (5, 4), (6, 5), (6, 4)] {
        let (tile, land) = tile(&sim, cell);
        assert!(TILES.is_edge_piece(tile), "{cell:?} {tile}");
        assert_eq!(land, LandType::Water.as_index(), "{cell:?}");
    }
    assert_eq!(tile(&sim, (4, 5)).1, LandType::Ice.as_index());
    for id in [1, 2] {
        let sinking = sim.substrate.entities.get(id).unwrap().sinking;
        assert!(sinking.is_active() && sinking.is_ice(), "{id}");
    }

    // The hull drifts three leptons north and nine down per frame.
    assert!(!sim.tick_ship_sinking(2, &rules, None, &mut FrameEffects::default()));
    let after = position_world_coord(&sim.substrate.entities.get(2).unwrap().position);
    assert_eq!(
        (after.x, after.y, after.z),
        (before.x, before.y - 3, before.z - 9)
    );
}

/// VERA deviation (docs/ts-ice.md): a heavy hull on open water breaks
/// nothing, though the water and the ice beside it make a breakable
/// footprint.
#[test]
fn a_heavy_unit_on_open_water_breaks_no_ice() {
    let rules = rules();
    let mut sim = snow(true);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .set_ice_tile((5, 5), TILES.water, true);
    place(&mut sim, 1, "HEAVY", (5, 5));
    let rng = sim.scenario_rng.native_state_hex();
    arrive(&mut sim, &rules, 1);
    assert_eq!(sim.scenario_rng.native_state_hex(), rng);
    assert_eq!(tile(&sim, (5, 5)).0, TILES.water);
    assert_eq!(tile(&sim, (6, 4)).0, TILES.ice[0]);
    assert!(!sim.substrate.entities.get(1).unwrap().sinking.is_active());
}

#[test]
fn cracked_ice_freezes_once_its_solidify_frame_has_passed() {
    let rules = rules();
    let mut sim = snow(true);
    place(&mut sim, 1, "LIGHT", (5, 5));
    arrive(&mut sim, &rules, 1);
    assert_eq!(sim.ice_cracks.len(), 1);

    sim.session.set_binary_frame_for_test(510);
    sim.solidify_ice(&rules, None);
    assert!(TILES.is_cracked(tile(&sim, (5, 5)).0));
    assert_eq!(sim.ice_cracks.len(), 1);

    sim.session.set_binary_frame_for_test(511);
    sim.solidify_ice(&rules, None);
    let (frozen, land) = tile(&sim, (5, 5));
    assert!(TILES.is_crackable(frozen), "{frozen}");
    assert_eq!(land, LandType::Ice.as_index());
    assert!(sim.ice_cracks.is_empty());
}

#[test]
fn growth_refreezes_ice_water_each_time_its_timer_runs_out() {
    let rules = rules();
    let mut sim = snow(true);
    // An ice/water edge piece on a cell the map lets grow ice.
    let open = TILES.ice[0] + 0x20;
    let thaw = |sim: &mut Simulation| {
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        terrain.set_ice_tile((5, 5), open, true);
        terrain.recalc_ice_cell((5, 5));
        terrain.cell_mut_for_test(5, 5).unwrap().legacy_ice_byte = 1;
    };
    thaw(&mut sim);
    assert_eq!(tile(&sim, (5, 5)).1, LandType::Water.as_index());

    // The constructor's timer has run out: grow now, then every 900 frames.
    sim.tick_ice_growth(&rules, None);
    assert_eq!(tile(&sim, (5, 5)).1, LandType::Ice.as_index());

    thaw(&mut sim);
    sim.session.set_binary_frame_for_test(909);
    sim.tick_ice_growth(&rules, None);
    assert_eq!(tile(&sim, (5, 5)).0, open);

    sim.session.set_binary_frame_for_test(910);
    sim.tick_ice_growth(&rules, None);
    assert!(TILES.is_crackable(tile(&sim, (5, 5)).0));
}

#[test]
fn ice_lake_grizzly_cracks_the_crossing_and_rhino_sinks() {
    use crate::headless_scenario::{battle_session, load_session};
    use crate::sim::command::Command;
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

    let Some((root, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map = "vera-ice-lake.map";
    let launch = battle_session(map, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
    let mut scene = load_session(&root, map, 17, launch).expect("ordinary bundled skirmish");
    let player = scene.sim().session.current_house().unwrap();
    let owner = scene.sim().resolve(player).to_owned();
    let mut drive = |kind, from: (u16, u16), to: (u16, u16)| {
        let runtime = &mut scene.runtime;
        let id = runtime
            .simulation
            .spawn_object(
                kind,
                owner.as_str(),
                from.0,
                from.1,
                0,
                &runtime.resources.rules,
            )
            .unwrap();
        assert!(runtime.apply_command_for_test(
            &owner,
            &Command::Move {
                entity_id: id,
                target_rx: to.0,
                target_ry: to.1,
            }
        ));
        id
    };

    // Start on opposite dry banks so one tank's damage cannot affect the other.
    let crossing = (56, 72);
    let grizzly = drive("MTNK", (55, 72), crossing);
    let rhino = drive("HTNK", (74, 53), (63, 63));
    for _ in 0..180 {
        scene.tick();
        if !scene.sim().ice_cracks.is_empty()
            && scene
                .sim()
                .entities()
                .get(rhino)
                .is_some_and(|unit| unit.sinking.is_ice())
        {
            break;
        }
    }

    let sim = scene.sim();
    assert!(sim.ice_tiles().unwrap().is_cracked(tile(sim, crossing).0));
    assert!(!sim.entities().get(grizzly).unwrap().sinking.is_active());
    let rhino = sim.entities().get(rhino).expect("Rhino entering the water");
    assert!(rhino.sinking.is_ice());
    assert_eq!(
        tile(sim, (rhino.position.rx(), rhino.position.ry())).1,
        LandType::Water.as_index()
    );
}
