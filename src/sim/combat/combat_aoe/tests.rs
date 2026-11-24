use crate::sim::terrain_object::{TerrainObjectLifecycle, TerrainObjectState};

use super::*;
use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts};
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::{HouseDifficulty, HouseState};
use crate::sim::intern::{StringInterner, test_intern, test_interner};
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};
use crate::sim::world::Simulation;

const CELL_INSET_FIXTURE_RULES: &str = "[VehicleTypes]\n0=FIRER\n1=SPRAY\n2=NOAREA\n3=EMPTY\n4=TARGET\n\
         [WeaponTypes]\n0=AREA\n1=DIRECT\n[Projectiles]\n0=INVIS\n\
         [Warheads]\n0=WH\n[FIRER]\nSecondary=AREA\n\
         [SPRAY]\nSprayAttack=yes\nPrimary=AREA\nSecondary=DIRECT\n\
         [NOAREA]\nSecondary=DIRECT\n\
         [AREA]\nProjectile=INVIS\nWarhead=WH\nAreaFire=yes\n\
         [DIRECT]\nProjectile=INVIS\nWarhead=WH\nAreaFire=no\n\
         [INVIS]\n[WH]\nCellSpread=3\nCellInset=2\n";

fn cell_inset_fixture() -> (RuleSet, Simulation) {
    let ini = IniFile::from_str(CELL_INSET_FIXTURE_RULES);
    let rules = RuleSet::from_ini(&ini).expect("CellInset fixture rules parse");
    cell_inset_world(rules)
}

fn cell_inset_world(rules: RuleSet) -> (RuleSet, Simulation) {
    let mut world = Simulation::new();
    let entities = [
        (1, "FIRER", 10, 10),
        (2, "TARGET", 11, 10),
        (3, "TARGET", 12, 10),
        (4, "SPRAY", 10, 10),
        (5, "NOAREA", 10, 10),
        (6, "EMPTY", 10, 10),
    ];
    for (stable_id, type_id, rx, ry) in entities {
        let type_ref = world.interner.intern(type_id);
        let mut entity = GameEntity::test_default(stable_id, type_id, "House", rx, ry);
        entity.set_type_ref_for_test(type_ref);
        world.substrate.entities.insert(entity);
    }
    world.interner.bind_types(&rules);
    (rules, world)
}

fn hit_ids(hits: &[EntityDamageEvent]) -> Vec<u64> {
    hits.iter().map(|event| event.target_id).collect()
}

#[test]
fn techno_triggers_cell_inset_includes_threshold_and_uses_native_deploy_slot() {
    let (rules, world) = cell_inset_fixture();

    assert!(techno_triggers_cell_inset(
        &world,
        1,
        TargetKind::Entity(2),
        &rules,
    ));
    assert!(!techno_triggers_cell_inset(
        &world,
        1,
        TargetKind::Entity(3),
        &rules,
    ));
    assert!(techno_triggers_cell_inset(
        &world,
        4,
        TargetKind::Entity(2),
        &rules,
    ));

    let mut layers = crate::rules::native_processing::RulesLayerStack::new(IniFile::from_str(
        CELL_INSET_FIXTURE_RULES,
    ));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[WH]\nCellSpread=1e999\nCellInset=1e999\n"),
    );
    let nan_rules = RuleSet::from_rules_layers(&layers).expect("NaN threshold rules");
    let (nan_rules, nan_world) = cell_inset_world(nan_rules);
    assert!(techno_triggers_cell_inset(
        &nan_world,
        1,
        TargetKind::Entity(2),
        &nan_rules,
    ));
}

#[test]
fn techno_triggers_cell_inset_requires_area_fire_and_a_deploy_weapon() {
    let (rules, world) = cell_inset_fixture();

    assert!(!techno_triggers_cell_inset(
        &world,
        5,
        TargetKind::Entity(2),
        &rules,
    ));
    assert!(!techno_triggers_cell_inset(
        &world,
        6,
        TargetKind::Entity(2),
        &rules,
    ));
}

fn hit_id_distances(hits: &[EntityDamageEvent]) -> Vec<(u64, i32)> {
    hits.iter()
        .map(|event| (event.target_id, event.distance_leptons))
        .collect()
}

fn wall_aoe_fixture(
    cell_spread: &str,
    warhead_flags: &str,
) -> (RuleSet, WarheadType, OverlayTypeRegistry) {
    let ini = IniFile::from_str(&format!(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=WH\n\
         [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n\
         [WH]\nCellSpread={cell_spread}\n{warhead_flags}\n\
         [MTNK]\nStrength=300\n\
         [GASAND]\nWall=yes\nArmor=wood\nStrength=400\n\
         [CYCL]\nWall=yes\nArmor=wood\nStrength=400\n\
         [GAWALL]\nWall=yes\nArmor=concrete\nStrength=400\n"
    ));
    let art = IniFile::from_str(
        "[GASAND]\nDamageLevels=2\n\
         [CYCL]\nDamageLevels=2\n\
         [GAWALL]\nDamageLevels=3\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("wall AoE rules parse");
    let warhead = WarheadType::from_ini_section("WH", ini.section("WH").expect("WH section"));
    let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    (rules, warhead, registry)
}

/// Exact detonation coordinate at the impact cell's centre.
fn center_impact(z_leptons: i32) -> AoEAirImpact {
    AoEAirImpact {
        sub_x: CELL_CENTER_LEPTON,
        sub_y: CELL_CENTER_LEPTON,
        z_leptons,
    }
}

/// Native cell-list collection. `air_z` is the exact centre impact Z;
/// `None` takes the whole-Level adapter at level 0.
fn native_lists<'a>(
    occupancy: &'a OccupancyGrid,
    terrain: &'a mut ResolvedTerrainGrid,
    air_z: Option<i32>,
) -> AoELayerContext<'a> {
    AoELayerContext {
        occupancy: Some(occupancy),
        terrain: Some(terrain),
        air_impact: air_z.map(center_impact),
        ..AoELayerContext::default()
    }
}

/// Wall routing without cell lists, at the whole-Level ground impact.
fn wall_routing<'a>(
    terrain: &'a mut ResolvedTerrainGrid,
    overlays: &'a mut OverlayGrid,
    registry: &'a OverlayTypeRegistry,
    rng: &'a mut SimRng,
) -> AoELayerContext<'a> {
    AoELayerContext {
        terrain: Some(terrain),
        overlay_grid: Some(overlays),
        overlay_registry: Some(registry),
        scenario_rng: Some(rng),
        ..AoELayerContext::default()
    }
}

/// Native AddContent on the ground list: a nonbuilding at the head.
fn prepend_ground(occupancy: &mut OccupancyGrid, rx: u16, ry: u16, id: u64) {
    occupancy.add(
        rx,
        ry,
        id,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );
}

/// Native AddContent on the ground list: a Building at the tail.
fn append_building(occupancy: &mut OccupancyGrid, rx: u16, ry: u16, id: u64) {
    occupancy.add(
        rx,
        ry,
        id,
        MovementLayer::Ground,
        None,
        CellListInsertion::AppendBuilding,
    );
}

/// State the production ordered receiver commit (`commit_damage_events`)
/// keeps across calls; no house order, overlays or traces.
struct ReceiverCommit {
    houses: crate::sim::houses::Houses,
    alliances: HouseAllianceMap,
    main_rng: SimRng,
    scenario_rng: SimRng,
    handled_deaths: Vec<u64>,
    sounds: Option<Vec<crate::sim::world::SimSoundEvent>>,
}

impl ReceiverCommit {
    fn new() -> Self {
        Self {
            houses: crate::sim::houses::Houses::default(),
            alliances: HouseAllianceMap::new(),
            main_rng: SimRng::new(1),
            scenario_rng: SimRng::new(2),
            handled_deaths: Vec::new(),
            sounds: None,
        }
    }

    fn run(
        &mut self,
        hits: &[EntityDamageEvent],
        entities: &mut EntityStore,
        occupancy: &mut OccupancyGrid,
        rules: &RuleSet,
        interner: &mut StringInterner,
        terrain: &mut ResolvedTerrainGrid,
    ) -> (
        crate::sim::combat::DeathEffects,
        Vec<crate::sim::combat::UnderAttackEvent>,
    ) {
        crate::sim::combat::commit_damage_events(
            hits,
            entities,
            occupancy,
            rules,
            interner,
            &mut self.houses,
            &self.alliances,
            &mut self.main_rng,
            &mut self.scenario_rng,
            &mut self.handled_deaths,
            None,
            None,
            Some(terrain),
            0,
            &mut None,
            &mut self.sounds.as_mut(),
        )
    }
}

#[test]
fn wrapped_spread_collects_the_real_zero_cell_on_both_axes() {
    for transpose in [false, true] {
        let (_, _, _, rules, mut warhead, mut interner) = bridge_layer_test_fixture();
        warhead.cell_spread_f64 = 2.;
        let (impact, receiver, sub) = if transpose {
            (
                (20, u16::MAX),
                (20, 0),
                (CELL_CENTER_LEPTON, SimFixed::from_num(-1)),
            )
        } else {
            (
                (u16::MAX, 20),
                (0, 20),
                (SimFixed::from_num(-1), CELL_CENTER_LEPTON),
            )
        };
        let mut entities = EntityStore::new();
        entities.insert(GameEntity::test_default(
            7, "MTNK", "Soviet", receiver.0, receiver.1,
        ));
        let mut occupancy = OccupancyGrid::new();
        prepend_ground(&mut occupancy, receiver.0, receiver.1, 7);
        let mut terrain = crate::map::resolved_terrain::test_grid(32, 32, test_terrain_cell);
        let result = apply_aoe_damage(
            &mut entities,
            impact.0,
            impact.1,
            100,
            &warhead,
            &rules,
            &mut interner,
            "Americans",
            AoELayerContext {
                air_impact: Some(AoEAirImpact {
                    sub_x: sub.0,
                    sub_y: sub.1,
                    z_leptons: 0,
                }),
                ..native_lists(&occupancy, &mut terrain, None)
            },
        );
        assert_eq!(
            hit_ids(&result.hits),
            vec![7],
            "native WORD-1+1 reaches the real zero cell, transpose{transpose}"
        );
        assert!(result.hits[0].distance_leptons > 0 && result.hits[0].distance_leptons < 512);
    }
}

#[test]
fn one_sweep_routes_alias_callbacks_and_walls_through_retained_real_identity() {
    struct RestampDummy {
        visits: Vec<(u16, u16)>,
    }
    impl AoECellPrelude for RestampDummy {
        fn before_cell(
            &mut self,
            rx: u16,
            ry: u16,
            _: Option<&mut OverlayGrid>,
            _: Option<&OverlayTypeRegistry>,
            terrain: Option<&mut ResolvedTerrainGrid>,
            _: Option<&mut SimRng>,
            _: Option<&OccupancyGrid>,
        ) {
            self.visits.push((rx, ry));
            if (rx, ry) == (511, 0) {
                let cells = NativeCellQuery::canonical(terrain.unwrap());
                assert_eq!(cells.lookup((20, 20)), NativeCellIdentity::Dummy);
            }
        }
    }
    for native_lists in [false, true] {
        let (rules, warhead, registry) =
            wall_aoe_fixture(".5", "WallAbsoluteDestroyer=yes\nWall=yes");
        let interner = test_interner();
        let mut terrain = crate::map::resolved_terrain::test_grid(512, 2, test_terrain_cell);
        let occupancy = OccupancyGrid::new();
        let mut overlays = OverlayGrid::new(512, 2);
        overlays.place_overlay(511, 0, 0, 0);
        let mut rng = SimRng::new(0x489603);
        let mut prelude = RestampDummy { visits: Vec::new() };
        let result = apply_aoe_damage_with_terrain_and_scenario(
            &mut EntityStore::new(),
            0,
            1,
            1,
            &warhead,
            &rules,
            &interner,
            None,
            "Americans",
            AoELayerContext {
                occupancy: native_lists.then_some(&occupancy),
                terrain: Some(&mut terrain),
                overlay_grid: Some(&mut overlays),
                overlay_registry: Some(&registry),
                scenario_rng: Some(&mut rng),
                air_impact: Some(center_impact(0)),
                impact_z: 0,
            },
            None,
            false,
            Some(&mut prelude),
        );
        assert!(
            prelude.visits.contains(&(511, 0)),
            "west of0,1 aliases real511,0 for native and headless sweep"
        );
        assert!(prelude.visits.iter().all(|&(x, y)| x < 512 && y < 2));
        assert_eq!(
            overlays.cell(511, 0).overlay_id,
            None,
            "a callback's Dummy stamp cannot redirect the retained real wall"
        );
        assert!(result.wall_mutations.iter().any(|m| (m.rx, m.ry, m.kind)
            == (
                511,
                0,
                crate::sim::overlay_grid::WallMutationKind::DirectRemoved
            )));
    }
}

#[test]
fn dummy_sweep_keeps_shared_overlay_but_never_fabricates_grid_residents() {
    #[derive(Default)]
    struct ObserveDummy {
        grid_preludes: usize,
        dirty_steps: usize,
    }
    impl AoECellPrelude for ObserveDummy {
        fn before_cell(
            &mut self,
            _: u16,
            _: u16,
            _: Option<&mut OverlayGrid>,
            _: Option<&OverlayTypeRegistry>,
            _: Option<&mut ResolvedTerrainGrid>,
            _: Option<&mut SimRng>,
            _: Option<&OccupancyGrid>,
        ) {
            self.grid_preludes += 1;
        }
        fn wall_dirty_step(&mut self, _: WallDirtyStep, _: (u16, u16)) {
            self.dirty_steps += 1;
        }
    }
    for dummy_wall in [false, true] {
        let (rules, warhead, registry) =
            wall_aoe_fixture("0", "WallAbsoluteDestroyer=yes\nWall=yes");
        let mut interner = test_interner();
        let tree_ref = interner.intern("TREE01");
        let mut entities = EntityStore::new();
        entities.insert(GameEntity::test_default(7, "MTNK", "Soviet", 20, 20));
        let mut occupancy = OccupancyGrid::new();
        prepend_ground(&mut occupancy, 20, 20, 7);
        let mut terrain = crate::map::resolved_terrain::test_grid(32, 32, test_terrain_cell);
        terrain.test_set_native_allocated_cells(&[(1, 1)]);
        let dummy = terrain.shared_cell_dummy();
        if dummy_wall {
            dummy.write_overlay_identity_state(0, 0);
        }
        let mut overlays = OverlayGrid::new(32, 32);
        overlays.place_overlay(20, 20, 0, 0);
        let mut objects = crate::sim::terrain_object::TerrainObjectStore::default();
        objects.insert_constructed(TerrainObjectState::for_test(8, tree_ref, 20, 20), false);
        objects.add_to_cell(8, (20, 20));
        let mut rng = SimRng::new(0x489608);
        let mut prelude = ObserveDummy::default();
        let result = apply_aoe_damage_with_terrain_and_scenario(
            &mut entities,
            20,
            20,
            1,
            &warhead,
            &rules,
            &interner,
            None,
            "Americans",
            AoELayerContext {
                overlay_grid: Some(&mut overlays),
                overlay_registry: Some(&registry),
                scenario_rng: Some(&mut rng),
                ..native_lists(&occupancy, &mut terrain, Some(0))
            },
            Some(TerrainCollectionView { store: &objects }),
            false,
            Some(&mut prelude),
        );
        assert!(
            result.receivers.is_empty(),
            "Dummy nullE4/E8 cannot acquire grid-backed Techno/Terrain from its coordinate"
        );
        assert_eq!(
            overlays.cell(20, 20).overlay_id,
            Some(0),
            "a missing Cell does not target a fake rectangular-grid overlay"
        );
        assert_eq!(
            dummy.overlay_identity_state().0,
            -1,
            "modeled nonempty Dummy wall still executes the existing wall owner"
        );
        assert!(
            result.wall_mutations.is_empty(),
            "Dummy never exports a fake real-cell render mutation"
        );
        assert_eq!(
            prelude.grid_preludes, 0,
            "Dummy does not fabricate a grid-backed resource prelude"
        );
        assert_eq!(
            prelude.dirty_steps > 0,
            dummy_wall,
            "a nonempty Dummy wall keeps its existing native dirty transaction"
        );
        if dummy_wall {
            // Explicit boundary state, not a claimed stock producer of a
            // nonempty Dummy. Recursive lookups restamp it; the outer
            // sweep must still match another request for that allocation.
            dummy.write_overlay_identity_state(0, 0);
            entities
                .get_mut(7)
                .unwrap()
                .attack_target
                .set_for_test(Some(TargetKind::Cell(21, 21)));
            let wh = interner.intern("WH");
            let mut world = Simulation::with_seed(0x489608);
            world.interner = interner;
            world.substrate.entities = entities;
            world.substrate.occupancy = occupancy;
            world.resolved_terrain = Some(terrain);
            world.overlay_grid = Some(overlays);
            world.terrain_objects = objects;
            let result = super::super::world_receiver::collect_area(
                &mut world,
                &rules,
                Some(&registry),
                (20, 20),
                1,
                &warhead,
                true,
                (super::super::RAD_NO_ATTACKER, None, wh),
                Some(center_impact(0)),
                0,
                &mut crate::sim::world::FrameEffects::default(),
            );
            assert!(result.receivers.is_empty());
            assert_eq!(
                result.cell_target_detaches,
                vec![CellTargetDetach {
                    listener_id: 7,
                    restored: false,
                    cleared: true
                }]
            );
            assert_eq!(
                world
                    .substrate
                    .entities
                    .get(7)
                    .unwrap()
                    .attack_target
                    .target(),
                None
            );
            assert_eq!(
                world.overlay_grid.as_ref().unwrap().cell(20, 20).overlay_id,
                Some(0)
            );
        }
    }
}

#[test]
fn gsi_04_07_damage_signed_zero_returns_before_wall_rng_objects_and_detach() {
    let mut entities = EntityStore::new();
    let mut listener = GameEntity::test_default(20, "MTNK", "Americans", 8, 8);
    listener
        .attack_target
        .set_for_test(Some(TargetKind::Cell(8, 8)));
    entities.insert(listener);
    let mut interner = test_interner();
    let warhead_ref = interner.intern("BlastWH");

    // WallAbsoluteDestroyer on GAWALL, then an ordinary Wall on GASAND.
    for (warhead_flags, overlay, seed) in [
        ("WallAbsoluteDestroyer=yes\nWall=yes", 2, 17),
        ("Wall=yes", 0, 29),
    ] {
        let (rules, warhead, registry) = wall_aoe_fixture(".5", warhead_flags);
        let mut overlays = OverlayGrid::new(16, 16);
        overlays.place_overlay(8, 8, overlay, 0);
        let mut scenario_rng = SimRng::new(seed);
        let before = scenario_rng.state();
        let result = apply_aoe_damage(
            &mut entities,
            8,
            8,
            0,
            &warhead,
            &rules,
            &mut interner,
            (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
            AoELayerContext {
                overlay_grid: Some(&mut overlays),
                overlay_registry: Some(&registry),
                scenario_rng: Some(&mut scenario_rng),
                ..AoELayerContext::default()
            },
        );
        assert_eq!(overlays.cell(8, 8).overlay_id, Some(overlay));
        assert_eq!(scenario_rng.state(), before);
        assert!(result.hits.is_empty());
        assert!(result.wall_mutations.is_empty());
        assert!(result.cell_target_detaches.is_empty());
        assert!(matches!(
            entities.get(20).unwrap().attack_target.target(),
            Some(TargetKind::Cell(8, 8))
        ));
    }
}

#[test]
fn gsi_04_10_scenario_no_damage_returns_before_wall_rng_objects_and_detach() {
    let mut entities = EntityStore::new();
    let mut listener = GameEntity::test_default(20, "MTNK", "Americans", 8, 8);
    listener
        .attack_target
        .set_for_test(Some(TargetKind::Cell(8, 8)));
    entities.insert(listener);
    let mut interner = test_interner();
    let warhead_ref = interner.intern("BlastWH");
    let (rules, warhead, registry) = wall_aoe_fixture(".5", "WallAbsoluteDestroyer=yes\nWall=yes");
    let mut overlays = OverlayGrid::new(16, 16);
    overlays.place_overlay(8, 8, 2, 0);
    let mut scenario_rng = SimRng::new(17);
    let before = scenario_rng.state();

    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    let result = apply_aoe_damage_with_terrain_and_scenario(
        &mut entities,
        8,
        8,
        100,
        &warhead,
        &rules,
        &interner,
        Some(handles),
        (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
        AoELayerContext {
            overlay_grid: Some(&mut overlays),
            overlay_registry: Some(&registry),
            scenario_rng: Some(&mut scenario_rng),
            ..AoELayerContext::default()
        },
        None,
        true,
        None,
    );

    assert_eq!(overlays.cell(8, 8).overlay_id, Some(2));
    assert_eq!(scenario_rng.state(), before);
    assert!(result.receivers.is_empty());
    assert!(result.wall_mutations.is_empty());
    assert!(result.cell_target_detaches.is_empty());
    assert!(matches!(
        entities.get(20).unwrap().attack_target.target(),
        Some(TargetKind::Cell(8, 8))
    ));
}

#[test]
fn gsi_04_11_per_cell_ore_rng_precedes_later_wall_rng() {
    struct OreReseedDraw {
        cell: (u16, u16),
    }

    impl AoECellPrelude for OreReseedDraw {
        fn before_cell(
            &mut self,
            rx: u16,
            ry: u16,
            _overlay_grid: Option<&mut OverlayGrid>,
            _overlay_registry: Option<&OverlayTypeRegistry>,
            _terrain: Option<&mut ResolvedTerrainGrid>,
            scenario_rng: Option<&mut SimRng>,
            _occupancy: Option<&OccupancyGrid>,
        ) {
            if (rx, ry) == self.cell {
                let _ = scenario_rng
                    .expect("AoE fixture supplies scenario RNG")
                    .next_range_u32_inclusive(0, 7);
            }
        }
    }

    let (rules, warhead, registry) = wall_aoe_fixture("1", "Wall=yes");
    let mut interner = test_interner();
    let warhead_ref = interner.intern("WH");
    let offsets = cell_spread::splash_cells(warhead.cell_spread_f64);
    assert_eq!(offsets[0], (0, 0));
    let wall_offset = offsets[1];
    let wall_cell = (
        (8_i32 + i32::from(wall_offset.0)) as u16,
        (8_i32 + i32::from(wall_offset.1)) as u16,
    );
    let mut overlays = OverlayGrid::new(16, 16);
    let mut terrain = crate::sim::tiberium::test_support::flat_terrain(16, 16);
    overlays.place_overlay(wall_cell.0, wall_cell.1, 2, 0);
    let mut scenario_rng = SimRng::new(91);
    let mut prelude = OreReseedDraw { cell: (8, 8) };

    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    let _ = apply_aoe_damage_with_terrain_and_scenario(
        &mut EntityStore::new(),
        8,
        8,
        1,
        &warhead,
        &rules,
        &interner,
        Some(handles),
        (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
        wall_routing(&mut terrain, &mut overlays, &registry, &mut scenario_rng),
        None,
        false,
        Some(&mut prelude),
    );

    let mut expected_rng = SimRng::new(91);
    let _ = expected_rng.next_range_u32_inclusive(0, 7);
    let _ = expected_rng.next_range_u32_inclusive(0, 400);
    assert_eq!(scenario_rng.state(), expected_rng.state());
}

#[test]
fn gsi_04_07_damage_air_height_gate_and_exact_3d_falloff_boundaries() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n0=JUMP\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n0=AIR\n\
         [BuildingTypes]\n\
         [Warheads]\n0=BlastWH\n\
         [AIR]\nStrength=1000\nArmor=none\n\
         [JUMP]\nStrength=1000\nArmor=none\n\
         [BlastWH]\nCellSpread=1\nPercentAtMax=.25\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("3D air rules");
    let warhead = rules.warhead("BlastWH").unwrap().clone();
    let mut entities = EntityStore::new();
    for (stable_id, type_id, category, altitude) in [
        (1, "AIR", EntityCategory::Aircraft, 1),
        (2, "AIR", EntityCategory::Aircraft, 129),
        (3, "AIR", EntityCategory::Aircraft, 257),
        (4, "AIR", EntityCategory::Aircraft, 300),
        (5, "JUMP", EntityCategory::Infantry, 257),
    ] {
        let mut air = GameEntity::test_default(stable_id, type_id, "Soviet", 5, 5);
        air.set_category_for_test(category);
        air.health.set_for_test(1000);
        // Ordinary Aircraft41B920 reaches Object5F6B90: Object+74 must
        // be marked before the physical GetHeight threshold is queried.
        air.lifecycle.set_in_limbo_for_test(false);
        air.lifecycle.set_cell_marked_for_test(true);
        air.position.set_exact_z_leptons_for_test(Some(altitude));
        let mut locomotor = crate::sim::movement::locomotor::LocomotorState::from_object_type(
            rules.object(type_id).unwrap(),
            0,
        );
        locomotor.set_layer_for_test(MovementLayer::Air);
        air.locomotor = Some(locomotor);
        air = air
            .with_air_spatial_membership_for_test(Some(5 + 5 * 20), stable_id)
            .with_air_tracker_cell_for_test((5, 5));
        entities.insert(air);
    }
    let mut interner = test_interner();
    let warhead_ref = interner.intern("BlastWH");
    let occupancy = OccupancyGrid::new();
    let mut terrain = crate::map::resolved_terrain::test_grid(20, 20, test_terrain_cell);

    let at_ground = apply_aoe_damage(
        &mut entities,
        5,
        5,
        100,
        &warhead,
        &rules,
        &mut interner,
        (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
        native_lists(&occupancy, &mut terrain, Some(0)),
    );
    assert!(
        at_ground.hits.is_empty(),
        "impact Z equal to exact ground height skips the entire air query"
    );

    let one_above = apply_aoe_damage(
        &mut entities,
        5,
        5,
        100,
        &warhead,
        &rules,
        &mut interner,
        (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
        native_lists(&occupancy, &mut terrain, Some(1)),
    );
    assert_eq!(
        hit_id_distances(&one_above.hits),
        vec![(1, 0), (2, 128), (3, 128), (5, 256)],
        "low Aircraft 129 and high Aircraft 257/2 both capture 128; airborne Infantry stays unhalved and raw distance 299 stays outside"
    );
}

/// The airborne phase (`0x004893F2..0x00489410`) takes each member that
/// `FillCurrentVector` (`0x00412B40`) copies out of the AircraftTracker
/// when it is alive (`+0x90`), marked (`+0x74`) and has Health left,
/// whatever its layer or height. An aircraft the tracker took at takeoff
/// while it was still on the ground is hit, and an unmarked member is not.
#[test]
fn gsi_04_07_damage_air_phase_takes_tracker_members_by_object_state() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n0=AIR\n[BuildingTypes]\n\
         [Warheads]\n0=BlastWH\n\
         [AIR]\nStrength=1000\nArmor=none\n\
         [BlastWH]\nCellSpread=1\nPercentAtMax=.25\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("air phase rules");
    let warhead = rules.warhead("BlastWH").unwrap().clone();
    let mut entities = EntityStore::new();
    for (stable_id, marked, layer) in [
        (1, true, MovementLayer::Ground),
        (2, false, MovementLayer::Air),
    ] {
        let mut air = GameEntity::test_default(stable_id, "AIR", "Soviet", 5, 5);
        air.set_category_for_test(EntityCategory::Aircraft);
        air.health.set_for_test(1000);
        air.lifecycle.set_in_limbo_for_test(false);
        air.lifecycle.set_cell_marked_for_test(marked);
        air.position.set_exact_z_leptons_for_test(Some(0));
        let mut locomotor = crate::sim::movement::locomotor::LocomotorState::from_object_type(
            rules.object("AIR").unwrap(),
            0,
        );
        locomotor.set_layer_for_test(layer);
        air.locomotor = Some(locomotor);
        air = air
            .with_air_spatial_membership_for_test(Some(5 + 5 * 20), stable_id)
            .with_air_tracker_cell_for_test((5, 5));
        entities.insert(air);
    }
    let mut interner = test_interner();
    let warhead_ref = interner.intern("BlastWH");
    let occupancy = OccupancyGrid::new();
    let mut terrain = crate::map::resolved_terrain::test_grid(20, 20, test_terrain_cell);

    let result = apply_aoe_damage(
        &mut entities,
        5,
        5,
        100,
        &warhead,
        &rules,
        &mut interner,
        (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
        native_lists(&occupancy, &mut terrain, Some(1)),
    );
    assert_eq!(hit_id_distances(&result.hits), vec![(1, 1)]);
}

/// Phobos `MapClass_DamageArea_AirDamageSelfFix @ 0x00489416`: retail's
/// airborne phase admits the source unconditionally; under Phobos it
/// keeps the source only for a `DamageSelf` type or an
/// `AllowDamageOnSelf` warhead.
#[test]
fn gsi_04_07_damage_air_source_admission_follows_damage_self() {
    fn run(damage_self: bool, warhead_name: &str) -> Vec<u64> {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n0=AIR\n\
             [BuildingTypes]\n\
             [Warheads]\n0=BlastWH\n1=AllowWH\n\
             [AIR]\nStrength=1000\nArmor=none\nDamageSelf={}\n\
             [BlastWH]\nCellSpread=1\nPercentAtMax=.25\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [AllowWH]\nCellSpread=1\nPercentAtMax=.25\nAllowDamageOnSelf=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            if damage_self { "yes" } else { "no" }
        ));
        let rules = RuleSet::from_ini(&ini).expect("air source admission rules");
        let warhead = rules.warhead(warhead_name).unwrap().clone();
        let mut interner = test_interner();
        let air_type = interner.intern("AIR");
        let warhead_ref = interner.intern(warhead_name);

        let mut air = GameEntity::test_default(1, "AIR", "Soviet", 5, 5);
        air.set_type_ref_for_test(air_type);
        air.set_category_for_test(EntityCategory::Aircraft);
        air.health.set_for_test(1000);
        air.lifecycle.set_in_limbo_for_test(false);
        air.lifecycle.set_cell_marked_for_test(true);
        air.position.set_exact_z_leptons_for_test(Some(1));
        let mut locomotor = crate::sim::movement::locomotor::LocomotorState::from_object_type(
            rules.object("AIR").unwrap(),
            0,
        );
        locomotor.set_layer_for_test(MovementLayer::Air);
        air.locomotor = Some(locomotor);
        let mut entities = EntityStore::new();
        entities.insert(
            air.with_air_spatial_membership_for_test(Some(5 + 5 * 20), 1)
                .with_air_tracker_cell_for_test((5, 5)),
        );
        let occupancy = OccupancyGrid::new();
        let mut terrain = crate::map::resolved_terrain::test_grid(20, 20, test_terrain_cell);
        let result = apply_aoe_damage(
            &mut entities,
            5,
            5,
            100,
            &warhead,
            &rules,
            &mut interner,
            (1, warhead_ref),
            native_lists(&occupancy, &mut terrain, Some(1)),
        );
        hit_ids(&result.hits)
    }

    // Retail admits the airborne source unconditionally.
    #[cfg(not(feature = "phobos"))]
    {
        assert_eq!(run(false, "BlastWH"), vec![1]);
        assert_eq!(run(true, "BlastWH"), vec![1]);
    }

    #[cfg(feature = "phobos")]
    {
        assert!(
            run(false, "BlastWH").is_empty(),
            "DamageSelf=no source is skipped in the airborne phase"
        );
        assert_eq!(run(true, "BlastWH"), vec![1], "DamageSelf=yes admits it");
        assert_eq!(
            run(false, "AllowWH"),
            vec![1],
            "AllowDamageOnSelf admits it"
        );
    }
}

#[test]
fn gsi_04_07_damage_ground_native_distance_and_receiver_kernel() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=IvanWH\n\
         [CombatDamage]\nMaxDamage=10000\n\
         [MTNK]\nStrength=300\nArmor=heavy\n\
         [IvanWH]\nCellSpread=1.5\nPercentAtMax=.25\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("Ivan falloff fixture");
    let warhead = rules.warhead("IvanWH").unwrap().clone();

    let mut entities = EntityStore::new();
    for (stable_id, rx, sub_x) in [(320, 11, 192), (384, 12, 0), (385, 12, 1), (386, 12, 2)] {
        let mut target = GameEntity::test_default(stable_id, "MTNK", "Soviet", rx, 10);
        target
            .position
            .set_sub_x_for_test(SimFixed::from_num(sub_x));
        target.position.set_sub_y_for_test(CELL_CENTER_LEPTON);
        target.health.set_for_test(300);
        entities.insert(target);
    }
    let mut interner = test_interner();
    let warhead_ref = interner.intern("IvanWH");
    let mut occupancy = OccupancyGrid::new();
    prepend_ground(&mut occupancy, 11, 10, 320);
    // Prepend in reverse so the cell's native selected-list order is
    // 384,385,386. The last candidate is excluded only after Sqrt_Approx.
    for stable_id in [386, 385, 384] {
        prepend_ground(&mut occupancy, 12, 10, stable_id);
    }
    let mut terrain = crate::map::resolved_terrain::test_grid(24, 24, test_terrain_cell);

    let result = apply_aoe_damage(
        &mut entities,
        10,
        10,
        450,
        &warhead,
        &rules,
        &mut interner,
        (77, warhead_ref),
        native_lists(&occupancy, &mut terrain, Some(0)),
    );
    assert_eq!(
        result
            .hits
            .iter()
            .map(|event| {
                (
                    event.target_id,
                    event.damage,
                    event.distance_leptons,
                    event.attacker_id,
                    event.warhead_ref,
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (320, 450, 320, 77, warhead_ref),
            (384, 450, 384, 77, warhead_ref),
            (385, 450, 384, 77, warhead_ref),
        ],
        "Sqrt_Approx keeps 385 at the inclusive boundary and rejects 386"
    );

    let (death, pings) = ReceiverCommit::new().run(
        &result.hits,
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut terrain,
    );
    assert!(death.despawned_ids.is_empty());
    assert!(pings.is_empty());
    assert_eq!(entities.get(320).unwrap().health.current(), 132);
    assert_eq!(entities.get(384).unwrap().health.current(), 188);
    assert_eq!(entities.get(385).unwrap().health.current(), 188);
    assert_eq!(entities.get(386).unwrap().health.current(), 300);
}

#[test]
fn gsi_04_07_damage_center_building_high_z_uses_two_level_allowance() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=BUILD\n\
         [Warheads]\n0=BlastWH\n\
         [BUILD]\nStrength=300\nArmor=concrete\n\
         [BlastWH]\nCellSpread=1\nPercentAtMax=.25\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("center-building height fixture");
    let warhead = rules.warhead("BlastWH").unwrap().clone();
    let mut interner = test_interner();
    let warhead_ref = interner.intern("BlastWH");
    let mut entities = EntityStore::new();
    let mut building = GameEntity::test_default_of_category(
        1,
        "BUILD",
        "Neutral",
        5,
        5,
        EntityCategory::Structure,
    );
    building.health.set_for_test(300);
    entities.insert(building);
    let mut occupancy = OccupancyGrid::new();
    append_building(&mut occupancy, 5, 5, 1);
    let mut terrain = crate::map::resolved_terrain::test_grid(12, 12, test_terrain_cell);

    for (impact_z, expected_distance) in [(208, 0), (256, 48)] {
        let result = apply_aoe_damage(
            &mut entities,
            5,
            5,
            100,
            &warhead,
            &rules,
            &mut interner,
            (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
            native_lists(&occupancy, &mut terrain, Some(impact_z)),
        );
        assert_eq!(
            result
                .hits
                .iter()
                .map(|event| (event.target_id, event.damage, event.distance_leptons))
                .collect::<Vec<_>>(),
            vec![(1, 100, expected_distance)],
            "center Building at impact Z {impact_z} keeps native two-Level allowance"
        );
    }
}

#[test]
fn gsi_04_07_damage_near_center_iron_curtain_isolates_non_invulnerable_records() {
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};

    fn run(protected_distance: i32, kind: InvulnKind) -> (Vec<(u64, i32, bool)>, i32, Vec<u64>) {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=TARGET\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=BlastWH\n\
             [CombatDamage]\nMaxDamage=10000\n\
             [TARGET]\nStrength=100\nArmor=heavy\n\
             [BlastWH]\nCellSpread=.5\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("near-center IC fixture");
        let warhead = rules.warhead("BlastWH").unwrap().clone();
        let mut interner = test_interner();
        let target_ref = interner.intern("TARGET");
        let victim_owner = interner.intern("Victim");
        let warhead_ref = interner.intern("BlastWH");
        let mut entities = EntityStore::new();

        let mut vulnerable = GameEntity::test_default(1, "TARGET", "Victim", 5, 5);
        vulnerable.set_type_ref_for_test(target_ref);
        vulnerable.set_owner_for_test(victim_owner);
        vulnerable.position.set_sub_x_for_test(CELL_CENTER_LEPTON);
        vulnerable.position.set_sub_y_for_test(CELL_CENTER_LEPTON);
        vulnerable.lifecycle.set_in_limbo_for_test(false);
        vulnerable.lifecycle.set_cell_marked_for_test(true);
        entities.insert(vulnerable);

        let mut protected = GameEntity::test_default(2, "TARGET", "Victim", 5, 5);
        protected.set_type_ref_for_test(target_ref);
        protected.set_owner_for_test(victim_owner);
        protected.position.set_sub_x_for_test(SimFixed::from_num(
            CELL_CENTER_LEPTON
                .to_num::<i32>()
                .wrapping_add(protected_distance),
        ));
        protected.position.set_sub_y_for_test(CELL_CENTER_LEPTON);
        protected.lifecycle.set_in_limbo_for_test(false);
        protected.lifecycle.set_cell_marked_for_test(true);
        protected.invulnerability = Some(InvulnerabilityState::new(
            crate::sim::timer::CdTimer::started(0, 100),
            kind,
        ));
        entities.insert(protected);

        let mut occupancy = OccupancyGrid::new();
        // Native non-building insertion is at the list head. Insert the
        // protected target first so the vulnerable record is collected
        // first, proving that a later IC record isolates globally.
        for stable_id in [2, 1] {
            prepend_ground(&mut occupancy, 5, 5, stable_id);
        }
        let mut terrain = crate::map::resolved_terrain::test_grid(12, 12, test_terrain_cell);
        let aoe = apply_aoe_damage(
            &mut entities,
            5,
            5,
            10,
            &warhead,
            &rules,
            &mut interner,
            (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
            native_lists(&occupancy, &mut terrain, Some(0)),
        );
        let records = aoe
            .hits
            .iter()
            .map(|event| {
                (
                    event.target_id,
                    event.distance_leptons,
                    event.near_center_ic_isolation_eligible,
                )
            })
            .collect::<Vec<_>>();

        let (effects, pings) = ReceiverCommit::new().run(
            &aoe.hits,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut terrain,
        );
        assert!(pings.is_empty());
        (
            records,
            entities.get(1).unwrap().health.current(),
            effects
                .combat_light_requests
                .iter()
                .filter_map(|effect| match effect {
                    crate::sim::combat::CombatLightRequest::Impact { target_id, .. } => *target_id,
                    crate::sim::combat::CombatLightRequest::Spark { .. } => None,
                })
                .collect(),
        )
    }

    for distance in [0, 84] {
        let (records, vulnerable_hp, effects) = run(distance, InvulnKind::IronCurtain);
        assert_eq!(records, vec![(1, 0, true), (2, distance, true)]);
        assert_eq!(
            vulnerable_hp, 100,
            "IC at signed distance {distance} isolates an earlier vulnerable record"
        );
        assert_eq!(
            effects,
            vec![2],
            "the protected receiver still runs in order"
        );
    }

    let (records, vulnerable_hp, effects) = run(85, InvulnKind::IronCurtain);
    assert_eq!(records, vec![(1, 0, true), (2, 85, true)]);
    assert_eq!(vulnerable_hp, 90, "distance 85 does not arm isolation");
    assert_eq!(effects, vec![2]);

    let (records, vulnerable_hp, effects) = run(0, InvulnKind::ForceShield);
    assert_eq!(records, vec![(1, 0, true), (2, 0, true)]);
    assert_eq!(vulnerable_hp, 90, "Force Shield does not arm isolation");
    assert_eq!(effects, vec![2]);
}

#[test]
fn gsi_04_07_damage_air_receivers_precede_center_first_ground_death_effects() {
    fn run(
        hp: i32,
    ) -> (
        Vec<u64>,
        crate::sim::combat::DeathEffects,
        OverlayGrid,
        EntityStore,
        u64,
    ) {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=GROUNDBOMB\n\
             [AircraftTypes]\n0=AIRBOMB\n\
             [BuildingTypes]\n\
             [Warheads]\n0=BlastWH\n1=WallWH\n\
             [OverlayTypes]\n0=TESTWALL\n\
             [AIRBOMB]\nStrength=10\nArmor=heavy\nExplodes=yes\nDeathWeapon=DeathBoom\n\
             [GROUNDBOMB]\nStrength=10\nArmor=heavy\nExplodes=yes\nDeathWeapon=DeathBoom\n\
             [DeathBoom]\nDamage=399\nWarhead=WallWH\n\
             [BlastWH]\nCellSpread=2\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [WallWH]\nCellSpread=0\nWall=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
        );
        let art = IniFile::from_str("[TESTWALL]\nDamageLevels=2\n");
        let rules = RuleSet::from_ini(&ini).expect("air-first death rules");
        let blast = rules.warhead("BlastWH").unwrap().clone();
        let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));

        let make_air = |stable_id, rx, enter_order| {
            let mut air = GameEntity::test_default(stable_id, "AIRBOMB", "Soviet", rx, 5);
            air.set_category_for_test(EntityCategory::Aircraft);
            air.lifecycle.set_in_limbo_for_test(false);
            // On the map, so marked (`+0x74`): the airborne phase skips
            // an unmarked member.
            air.lifecycle.set_cell_marked_for_test(true);
            air.health.set_for_test(hp);
            air.position.set_z_for_test(0);
            let mut air_locomotor =
                crate::sim::movement::locomotor::LocomotorState::from_object_type(
                    rules.object("AIRBOMB").unwrap(),
                    0,
                );
            air_locomotor.set_layer_for_test(MovementLayer::Air);
            air.locomotor = Some(air_locomotor);
            air.position.set_exact_z_leptons_for_test(Some(1));
            air = air
                .with_air_spatial_membership_for_test(Some(rx + 5 * 20), enter_order)
                .with_air_tracker_cell_for_test((rx as i16, 5));
            air
        };
        // Same-bucket vector order deliberately reverses stable IDs. The
        // lower-ID perimeter aircraft must still wait for the center bucket.
        let air_center_first = make_air(40, 5, 1);
        let air_center_second = make_air(30, 5, 2);
        let air_perimeter = make_air(5, 3, 3);

        let mut center = GameEntity::test_default(20, "GROUNDBOMB", "Soviet", 5, 5);
        center.lifecycle.set_in_limbo_for_test(false);
        center.health.set_for_test(hp);
        let mut east = GameEntity::test_default(10, "GROUNDBOMB", "Soviet", 7, 5);
        east.lifecycle.set_in_limbo_for_test(false);
        east.health.set_for_test(hp);

        let mut entities = EntityStore::new();
        entities.insert(east);
        entities.insert(center);
        entities.insert(air_perimeter);
        entities.insert(air_center_second);
        entities.insert(air_center_first);
        let mut occupancy = OccupancyGrid::new();
        prepend_ground(&mut occupancy, 5, 5, 20);
        prepend_ground(&mut occupancy, 7, 5, 10);
        let mut terrain = crate::map::resolved_terrain::test_grid(10, 10, test_terrain_cell);
        let mut overlays = OverlayGrid::new(10, 10);
        for &(rx, ry) in &[(3, 5), (5, 5), (7, 5)] {
            overlays.place_overlay(rx, ry, 0, 0);
        }

        let mut interner = test_interner();
        let _handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
        let blast_ref = interner.intern("BlastWH");
        let mut scenario_rng = SimRng::new(1);
        let aoe = apply_aoe_damage(
            &mut entities,
            5,
            5,
            10,
            &blast,
            &rules,
            &mut interner,
            (crate::sim::combat::RAD_NO_ATTACKER, blast_ref),
            AoELayerContext {
                overlay_grid: Some(&mut overlays),
                overlay_registry: Some(&registry),
                scenario_rng: Some(&mut scenario_rng),
                ..native_lists(&occupancy, &mut terrain, Some(1))
            },
        );
        let hit_order = hit_ids(&aoe.hits);
        let mut main_rng = SimRng::new(9);
        let mut handled_deaths = Vec::new();
        let mut houses = crate::sim::houses::Houses::default();
        let mut fatal_lifecycle = None;
        let mut sound_sink = None;
        // The production Apply_area_damage commit, whose dispatch gates
        // re-read each record's object when its turn comes.
        let (effects, pings) = crate::sim::combat::receiver_fixture::commit_area_damage_receivers(
            &aoe.receivers,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            Some(&mut overlays),
            Some(&registry),
            Some(&mut terrain),
            None,
            0,
            &mut fatal_lifecycle,
            &mut sound_sink,
        );
        assert!(pings.is_empty());
        (hit_order, effects, overlays, entities, scenario_rng.state())
    }

    let (hit_order, fatal, fatal_walls, fatal_entities, fatal_rng) = run(10);
    assert_eq!(
        hit_order,
        vec![40, 30, 5, 20, 10],
        "center air bucket/vector order, perimeter air, then center-first ground"
    );
    assert_eq!(
        fatal.immediate_uninit_ids,
        vec![30, 40, 5, 20, 10],
        // Aircraft4165C0 calls Foot at4165EC before its concrete cleanup.
        // Nested30 therefore finishes before40. The captured outer30 record
        // is then skipped: Apply_area_damage's dispatch reads its Health
        // (0) at 0x00489A79 before calling the receiver.
        "nested aircraft cleanup precedes its parent; the dead outer record is not dispatched"
    );
    let direct_cells: Vec<_> = fatal
        .wall_mutations
        .iter()
        .filter(|mutation| {
            matches!(
                mutation.kind,
                crate::sim::overlay_grid::WallMutationKind::DirectUpdated
                    | crate::sim::overlay_grid::WallMutationKind::DirectRemoved
            )
        })
        .map(|mutation| (mutation.rx, mutation.ry))
        .collect();
    let mut reference_rng = SimRng::new(1);
    let rolls = [
        reference_rng.next_range_u32_inclusive(0, 400),
        reference_rng.next_range_u32_inclusive(0, 400),
        reference_rng.next_range_u32_inclusive(0, 400),
    ];
    assert_eq!(rolls[0], 213);
    let cells = [(5, 5), (3, 5), (7, 5)];
    let expected_direct: Vec<_> = cells
        .into_iter()
        .zip(rolls)
        .filter_map(|(cell, roll)| (roll < 399).then_some(cell))
        .collect();
    assert_eq!(direct_cells, expected_direct);
    assert_eq!(fatal_rng, reference_rng.state());
    for (cell, roll) in cells.into_iter().zip(rolls) {
        assert_eq!(
            fatal_walls.cell(cell.0, cell.1).overlay_id.is_none(),
            roll < 399
        );
    }
    for id in [40, 30, 5, 20, 10] {
        assert_eq!(fatal_entities.get(id).unwrap().health.current(), 0);
    }

    let (hit_order, survives, surviving_walls, surviving_entities, surviving_rng) = run(11);
    assert_eq!(hit_order, vec![40, 30, 5, 20, 10]);
    assert!(survives.immediate_uninit_ids.is_empty());
    assert!(survives.wall_mutations.is_empty());
    assert_eq!(surviving_rng, SimRng::new(1).state());
    for &(rx, ry) in &cells {
        assert_eq!(surviving_walls.cell(rx, ry).overlay_id, Some(0));
    }
    for id in [40, 30, 5, 20, 10] {
        assert_eq!(surviving_entities.get(id).unwrap().health.current(), 1);
    }
}

/// Stock table only: Ares walks the CellSpreadEnumerator instead
/// (`cell_spread::area_damage_cells`, hooks `0x4895B8`..`0x4899BE`).
#[test]
fn gsi_04_07_damage_band11_repeated_cell_dispatches_receiver_twice() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=TESTUNIT\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=WIDEWH\n\
         [TESTUNIT]\nStrength=100\nArmor=heavy\n\
         [WIDEWH]\nCellSpread=11\nPercentAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("band-11 rules");
    let wide = rules.warhead("WIDEWH").unwrap().clone();
    let mut stock = wide.clone();
    stock.cell_spread_f64 = 10.0;

    let make_ground = |stable_id, rx, ry, sub_x, sub_y, hp| {
        let mut entity = GameEntity::test_default(stable_id, "TESTUNIT", "Soviet", rx, ry);
        entity
            .position
            .set_sub_x_for_test(SimFixed::from_num(sub_x));
        entity
            .position
            .set_sub_y_for_test(SimFixed::from_num(sub_y));
        entity.health.set_for_test(hp);
        entity
    };
    let mut entities = EntityStore::new();
    entities.insert(make_ground(100, 20, 20, 128, 128, 100));
    // Table indices 319 and 322 both visit (-3,+11). This exact
    // sub-cell coordinate is 2,763 leptons from the impact, within 2,816.
    entities.insert(make_ground(200, 17, 31, 255, 0, 60));
    entities.insert(make_ground(300, 23, 31, 0, 0, 100));
    entities.insert(make_ground(400, 17, 9, 255, 255, 100));

    let mut occupancy = OccupancyGrid::new();
    for &(id, rx, ry) in &[(100, 20, 20), (200, 17, 31), (300, 23, 31), (400, 17, 9)] {
        prepend_ground(&mut occupancy, rx, ry, id);
    }
    let mut terrain = crate::map::resolved_terrain::test_grid(48, 48, test_terrain_cell);
    let mut interner = test_interner();
    let _handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    let warhead_ref = interner.intern("WIDEWH");

    assert_eq!(
        cell_spread::splash_cells(wide.cell_spread_f64)
            .iter()
            .enumerate()
            .filter_map(|(index, &offset)| (offset == (-3, 11)).then_some(index))
            .collect::<Vec<_>>(),
        vec![319, 322]
    );
    let stock_result = apply_aoe_damage(
        &mut entities,
        20,
        20,
        30,
        &stock,
        &rules,
        &mut interner,
        (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
        native_lists(&occupancy, &mut terrain, None),
    );
    assert_eq!(hit_ids(&stock_result.hits), vec![100]);

    let result = apply_aoe_damage(
        &mut entities,
        20,
        20,
        30,
        &wide,
        &rules,
        &mut interner,
        (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
        native_lists(&occupancy, &mut terrain, None),
    );
    assert_eq!(
        hit_ids(&result.hits),
        vec![100, 200, 300, 400, 200],
        "indices 319/320/321/322 retain the repeated receiver in place"
    );

    let mut commit = ReceiverCommit {
        main_rng: SimRng::new(7),
        scenario_rng: SimRng::new(9),
        ..ReceiverCommit::new()
    };
    let (death, pings) = commit.run(
        &result.hits,
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut terrain,
    );
    assert!(pings.is_empty());
    assert_eq!(commit.handled_deaths, vec![200]);
    assert_eq!(death.immediate_uninit_ids, vec![200]);
    assert_eq!(entities.get(200).unwrap().health.current(), 0);
    assert_eq!(entities.get(100).unwrap().health.current(), 70);
    assert_eq!(entities.get(300).unwrap().health.current(), 70);
    assert_eq!(entities.get(400).unwrap().health.current(), 70);
}

/// Ares 3.0 DamageArea_CellSpread1/2/3 (`100531F0`/`10053250`/`10053270`,
/// hooks `0x4895B8`/`0x4895C7`/`0x4899BE`): the CellSpreadEnumerator's
/// first ring visits `(-1,-1)` before `(1,-1)`; the stock table visits
/// `(1,-1)` first.
#[test]
fn ares_area_damage_walks_the_cell_spread_enumerator_ring_order() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=TESTUNIT\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=RINGWH\n\
         [TESTUNIT]\nStrength=100\nArmor=heavy\n\
         [RINGWH]\nCellSpread=1\nPercentAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("ring rules");
    let warhead = rules.warhead("RINGWH").unwrap().clone();
    let make_ground = |stable_id, rx, ry, sub_x, sub_y| {
        let mut entity = GameEntity::test_default(stable_id, "TESTUNIT", "Soviet", rx, ry);
        entity
            .position
            .set_sub_x_for_test(SimFixed::from_num(sub_x));
        entity
            .position
            .set_sub_y_for_test(SimFixed::from_num(sub_y));
        entity
    };
    let mut entities = EntityStore::new();
    entities.insert(make_ground(100, 20, 20, 128, 128));
    // (1,-1) and (-1,-1), each about 182 leptons from the impact.
    entities.insert(make_ground(200, 21, 19, 0, 255));
    entities.insert(make_ground(300, 19, 19, 255, 255));
    let mut occupancy = OccupancyGrid::new();
    for &(id, rx, ry) in &[(100, 20, 20), (200, 21, 19), (300, 19, 19)] {
        prepend_ground(&mut occupancy, rx, ry, id);
    }
    let mut terrain = crate::map::resolved_terrain::test_grid(48, 48, test_terrain_cell);
    let mut interner = test_interner();
    let _handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    let warhead_ref = interner.intern("RINGWH");
    let result = apply_aoe_damage(
        &mut entities,
        20,
        20,
        30,
        &warhead,
        &rules,
        &mut interner,
        (crate::sim::combat::RAD_NO_ATTACKER, warhead_ref),
        native_lists(&occupancy, &mut terrain, None),
    );
    assert_eq!(hit_ids(&result.hits), vec![100, 300, 200]);
}

#[test]
fn gsi_04_07_damage_center_first_seeded_strength_sweep() {
    let (rules, warhead, registry) = wall_aoe_fixture(".5", "Wall=yes");
    let mut entities = EntityStore::new();
    let mut interner = test_interner();
    let mut overlays = OverlayGrid::new(16, 16);
    let mut terrain = crate::sim::tiberium::test_support::flat_terrain(16, 16);
    overlays.place_overlay(8, 8, 0, 0);
    overlays.place_overlay(9, 7, 0, 0);
    let mut scenario_rng = SimRng::new(1);

    let mut reference_rng = SimRng::new(1);
    let center_roll = reference_rng.next_range_u32_inclusive(0, 400);
    let diagonal_roll = reference_rng.next_range_u32_inclusive(0, 400);
    assert_eq!(center_roll, 213, "seed fixes the first scanned-cell roll");

    let result = apply_aoe_damage(
        &mut entities,
        8,
        8,
        214,
        &warhead,
        &rules,
        &mut interner,
        "Americans",
        wall_routing(&mut terrain, &mut overlays, &registry, &mut scenario_rng),
    );

    assert_eq!(scenario_rng.state(), reference_rng.state());
    assert_eq!(overlays.cell(8, 8).overlay_id, None);
    assert_eq!(
        overlays.cell(9, 7).overlay_id.is_none(),
        diagonal_roll < 214,
        "the second RNG result belongs to the second splash-table cell"
    );
    let direct_cells: Vec<(u16, u16)> = result
        .wall_mutations
        .iter()
        .filter(|mutation| {
            matches!(
                mutation.kind,
                crate::sim::overlay_grid::WallMutationKind::DirectUpdated
                    | crate::sim::overlay_grid::WallMutationKind::DirectRemoved
            )
        })
        .map(|mutation| (mutation.rx, mutation.ry))
        .collect();
    let mut expected = vec![(8, 8)];
    if diagonal_roll < 214 {
        expected.push((9, 7));
    }
    assert_eq!(direct_cells, expected);
}

#[test]
fn gsi_04_07_damage_fractional_spread_keeps_raw_i32_and_nuke_forces_all() {
    let (rules, wall, registry) = wall_aoe_fixture(".5", "Wall=yes");
    let mut entities = EntityStore::new();
    let mut interner = test_interner();
    let mut overlays = OverlayGrid::new(32, 32);
    let mut terrain = crate::sim::tiberium::test_support::flat_terrain(32, 32);
    overlays.place_overlay(16, 16, 0, 0);
    overlays.place_overlay(17, 15, 0, 0);
    let mut scenario_rng = SimRng::new(19);
    let before = scenario_rng.state();
    let result = apply_aoe_damage(
        &mut entities,
        16,
        16,
        65_537,
        &wall,
        &rules,
        &mut interner,
        "Americans",
        wall_routing(&mut terrain, &mut overlays, &registry, &mut scenario_rng),
    );
    assert_eq!(
        scenario_rng.state(),
        before,
        "raw 65537 must not wrap below Strength"
    );
    let direct_cells: Vec<(u16, u16)> = result
        .wall_mutations
        .iter()
        .filter(|mutation| {
            mutation.kind == crate::sim::overlay_grid::WallMutationKind::DirectRemoved
        })
        .map(|mutation| (mutation.rx, mutation.ry))
        .collect();
    assert_eq!(
        direct_cells,
        vec![(16, 16), (17, 15)],
        "CellSpread .5 reaches the adjacent wall and preserves center-first order"
    );

    let (rules, nuke, registry) = wall_aoe_fixture("10", "WallAbsoluteDestroyer=yes\nWall=yes");
    let mut overlays = OverlayGrid::new(40, 40);
    let mut terrain = crate::sim::tiberium::test_support::flat_terrain(40, 40);
    let stock_cells = [(20, 20), (20, 10), (30, 20), (17, 29)];
    for &(rx, ry) in &stock_cells {
        overlays.place_overlay(rx, ry, 2, 0);
    }
    let mut scenario_rng = SimRng::new(23);
    let before = scenario_rng.state();
    let result = apply_aoe_damage(
        &mut entities,
        20,
        20,
        -2,
        &nuke,
        &rules,
        &mut interner,
        "Americans",
        wall_routing(&mut terrain, &mut overlays, &registry, &mut scenario_rng),
    );
    assert_eq!(
        scenario_rng.state(),
        before,
        "WAD routes literal -1 on every spread cell"
    );
    for &(rx, ry) in &stock_cells {
        assert_eq!(overlays.cell(rx, ry).overlay_id, None);
        assert!(result.wall_mutations.iter().any(|mutation| {
            (mutation.rx, mutation.ry) == (rx, ry)
                && mutation.kind == crate::sim::overlay_grid::WallMutationKind::DirectRemoved
        }));
    }
}

#[test]
fn wall_cleanup_preserves_neighbor_targets_until_an_explicit_outer_sweep() {
    let (rules, warhead, registry) = wall_aoe_fixture("0", "WallAbsoluteDestroyer=yes\nWall=yes");
    let mut entities = EntityStore::new();
    let mut restored = GameEntity::test_default(20, "MTNK", "Americans", 2, 2);
    restored
        .attack_target
        .set_for_test(Some(TargetKind::Cell(8, 8)));
    restored.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(MissionType::Attack),
        suspended: MissionId::from_known(MissionType::Guard),
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    restored
        .seldom
        .suspended_attack_target
        .set_for_test(Some(TargetKind::Cell(8, 8)));
    entities.insert(restored);

    let mut cleared = GameEntity::test_default(30, "MTNK", "Americans", 3, 2);
    cleared
        .attack_target
        .set_for_test(Some(TargetKind::Cell(8, 8)));
    cleared
        .passive_scan
        .set_passively_acquired_target_for_test(true);
    entities.insert(cleared);

    let mut chain_only = GameEntity::test_default(40, "MTNK", "Americans", 4, 2);
    chain_only
        .attack_target
        .set_for_test(Some(TargetKind::Cell(8, 7)));
    entities.insert(chain_only);

    let owner = crate::sim::intern::InternedId::from_index(7);
    let mut overlays = OverlayGrid::new(16, 16);
    let terrain = crate::sim::tiberium::test_support::flat_terrain(16, 16);
    overlays.place_overlay(8, 8, 2, 0);
    overlays.place_owned_wall(8, 7, 2, 0x24, owner);
    let mut world = crate::sim::world::Simulation::with_seed(5);
    world.interner = test_interner();
    world.substrate.entities = entities;
    world.resolved_terrain = Some(terrain);
    world.overlay_grid = Some(overlays);
    let wh = world.interner.intern("WH");
    let result = super::super::world_receiver::collect_area(
        &mut world,
        &rules,
        Some(&registry),
        (8, 8),
        1,
        &warhead,
        true,
        (super::super::RAD_NO_ATTACKER, None, wh),
        Some(center_impact(0)),
        0,
        &mut crate::sim::world::FrameEffects::default(),
    );
    let overlays = world.overlay_grid.as_ref().unwrap();
    let entities = &world.substrate.entities;

    assert_eq!(overlays.cell(8, 8).overlay_id, None);
    assert_eq!(
        overlays.cell(8, 7).overlay_id,
        None,
        "cleanup created this removal"
    );
    assert_eq!(
        result.cell_target_detaches,
        vec![
            CellTargetDetach {
                listener_id: 30,
                restored: false,
                cleared: true
            },
            CellTargetDetach {
                listener_id: 20,
                restored: true,
                cleared: true
            },
        ],
        "only the impact cell receives70D4A0, after recursion, in descending order"
    );
    assert!(entities.get(30).unwrap().attack_target.target().is_none());
    assert!(!entities.get(30).unwrap().passive_scan.acquired());
    assert!(entities.get(20).unwrap().attack_target.target().is_none());
    assert_eq!(
        entities.get(40).unwrap().attack_target.target(),
        Some(TargetKind::Cell(8, 7))
    );
}

/// AreaDamage restores before conditionally clearing the Cell target.
/// Excluding Terrain receivers must not remove live Terrain from target admission.
#[test]
fn cell_expiry_restores_terrain_target_independently_of_damage_collection() {
    use crate::sim::combat::receiver_fixture::{FixturePolicy, FixtureTrace};
    use crate::sim::world::Simulation;

    for (label, health, lifecycle, present, collect_terrain, admitted) in [
        (
            "live excluded",
            100,
            TerrainObjectLifecycle::Live,
            true,
            false,
            true,
        ),
        (
            "live included",
            100,
            TerrainObjectLifecycle::Live,
            true,
            true,
            true,
        ),
        (
            "zero health",
            0,
            TerrainObjectLifecycle::Live,
            true,
            false,
            false,
        ),
        (
            "limbo",
            100,
            TerrainObjectLifecycle::Limbo,
            true,
            false,
            true,
        ),
        (
            "uninitialized",
            100,
            TerrainObjectLifecycle::Destroyed,
            true,
            false,
            false,
        ),
        (
            "missing",
            100,
            TerrainObjectLifecycle::Live,
            false,
            false,
            false,
        ),
    ] {
        let (rules, warhead, registry) =
            wall_aoe_fixture("1", "WallAbsoluteDestroyer=yes\nWall=yes");
        let mut world = Simulation::with_seed(5);
        world.interner = test_interner();
        world.resolved_terrain = Some(crate::sim::tiberium::test_support::flat_terrain(16, 16));
        let mut overlays = OverlayGrid::new(16, 16);
        overlays.place_overlay(8, 8, 2, 0);
        world.overlay_grid = Some(overlays);
        world.receiver_fixture = Some(FixturePolicy {
            current_tick: 0,
            require_playfield_membership: false,
            fog_enabled: false,
            radiation_enabled: false,
            sound_enabled: false,
            terrain_collection: collect_terrain,
            trace: FixtureTrace::default(),
            deferred_tiberium: Vec::new(),
            tail_effects: Vec::new(),
            admitted_spawns: Vec::new(),
        });

        let saved = Some(TargetKind::Entity(100));
        let mut listener = GameEntity::test_default(20, "MTNK", "Americans", 2, 2);
        listener
            .attack_target
            .set_for_test(Some(TargetKind::Cell(8, 8)));
        listener.seldom.suspended_attack_target.set_for_test(saved);
        listener.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Attack),
            suspended: MissionId::from_known(MissionType::Guard),
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        world.substrate.entities.insert(listener);
        if present {
            // Explicit admission controls, not a simulated death/limbo
            // transition. Terrain remains in its real owning store.
            let type_ref = world.interner.intern("TREE01");
            let mut tree = TerrainObjectState::for_test(100, type_ref, 9, 8);
            tree.set_health_for_test(health);
            tree.set_lifecycle_for_test(lifecycle);
            world
                .terrain_objects
                .objects_mut_for_test()
                .insert(100, tree);
            world
                .terrain_objects
                .cell_index_mut_for_test()
                .insert((9, 8), 100);
        }
        let warhead_ref = world.interner.intern("WH");
        let result = super::super::world_receiver::collect_area(
            &mut world,
            &rules,
            Some(&registry),
            (8, 8),
            1,
            &warhead,
            true,
            (super::super::RAD_NO_ATTACKER, None, warhead_ref),
            Some(center_impact(0)),
            0,
            &mut crate::sim::world::FrameEffects::default(),
        );

        assert_eq!(
            world.overlay_grid.as_ref().unwrap().cell(8, 8).overlay_id,
            None,
            "{label}"
        );
        let listener = world.substrate.entities.get(20).unwrap();
        assert_eq!(
            listener.attack_target.target(),
            admitted.then_some(saved.unwrap()),
            "{label}"
        );
        assert_eq!(
            listener.mission.current(),
            MissionId::from_known(MissionType::Guard),
            "{label}"
        );
        assert_eq!(listener.mission.suspended(), MissionId::NONE, "{label}");
        assert_eq!(
            result.cell_target_detaches,
            vec![CellTargetDetach {
                listener_id: 20,
                restored: true,
                cleared: false,
            }],
            "{label}: Restore replaces the Cell target before the conditional clear"
        );
        assert_eq!(
            result.receivers.iter().any(|receiver| matches!(
                receiver, AreaDamageReceiver::Terrain(event) if event.stable_id == 100
            )),
            collect_terrain,
            "{label}: receiver inclusion is independent of target admission"
        );
    }
}

#[test]
fn bridge_impact_selects_the_deck_list_strictly_above_half_deck() {
    // Apply_area_damage48955E..48958D compares the exact impact against
    // Map578080's floor plus half of the original416-lepton deck (208);
    // strict `>` keeps the boundary on the ground list (entity 1), above
    // it only the deck list (entity 2) is hit. Without an exact coordinate
    // the whole-Level adapter lifts `impact_z` (level 2 is exactly 208).
    // With one, the caller's whole-Level fallback is unread: 209 must not
    // be rounded down to the 208 boundary.
    for (air_z, impact_z, expected) in [
        (None, 2, 1),
        (None, 4, 2),
        (Some(208), 2, 1),
        (Some(209), 2, 2),
    ] {
        let (mut entities, occupancy, mut terrain, rules, warhead, mut interner) =
            bridge_layer_test_fixture();
        let hits = apply_aoe_damage(
            &mut entities,
            5,
            5,
            100,
            &warhead,
            &rules,
            &mut interner,
            "Americans",
            AoELayerContext {
                impact_z,
                ..native_lists(&occupancy, &mut terrain, air_z)
            },
        )
        .hits;
        assert_eq!(
            hit_ids(&hits),
            vec![expected],
            "exact Z {air_z:?}, level {impact_z}"
        );
    }
}

#[test]
fn aoe_object_list_switches_at_halfway_to_the_retained_deck() {
    use crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF;

    let (mut entities, occupancy, mut terrain, rules, warhead, mut interner) =
        bridge_layer_test_fixture();
    let cell = terrain.native_cell_identity((5, 5));
    terrain.write_native_cell_flags(cell, BRIDGE_FLAG_ANCHOR_SELF | BRIDGE_FLAG_STRUCTURAL);
    terrain.set_authored_high_bridge_deck(cell, 9);

    for (z, layer) in [(468, ListLayer::Ground), (469, ListLayer::Bridge)] {
        let result = apply_aoe_damage(
            &mut entities,
            5,
            5,
            100,
            &warhead,
            &rules,
            &mut interner,
            "Americans",
            native_lists(&occupancy, &mut terrain, Some(z)),
        );
        assert_eq!(result.object_list_layer, layer);
    }
}

#[test]
fn bridge_object_layer_uses_signed_slope_floor_at_exact_subcell_xy() {
    // Reuse the established Cell47B3A0 controls from util::lepton: level2
    // ramp1 at(64,192) gives234; signed levelFF ramp1 at(1,0) gives-102.
    for (level, sub_x, sub_y, floor) in [(2, 64, 192, 234), (0xff, 1, 0, -102)] {
        for (above, expected) in [(208, ListLayer::Ground), (209, ListLayer::Bridge)] {
            let (mut entities, occupancy, mut terrain, rules, warhead, mut interner) =
                bridge_layer_test_fixture();
            let cell = terrain.cell_mut_for_test(5, 5).unwrap();
            cell.level = level;
            cell.slope_type = 1;
            let z = floor + above;
            let result = apply_aoe_damage(
                &mut entities,
                5,
                5,
                100,
                &warhead,
                &rules,
                &mut interner,
                "Americans",
                AoELayerContext {
                    air_impact: Some(AoEAirImpact {
                        sub_x: SimFixed::from_num(sub_x),
                        sub_y: SimFixed::from_num(sub_y),
                        z_leptons: z,
                    }),
                    impact_z: z.div_euclid(LEPTONS_PER_LEVEL as i32),
                    ..native_lists(&occupancy, &mut terrain, None)
                },
            );
            assert_eq!(
                result.object_list_layer, expected,
                "level{level} sub{sub_x},{sub_y} Z{z}"
            );
        }
    }
}

#[test]
fn bridge_object_layer_is_frozen_before_any_cell_prelude() {
    struct ChangeBridge {
        flags: u32,
        visited: bool,
    }
    impl AoECellPrelude for ChangeBridge {
        fn before_cell(
            &mut self,
            rx: u16,
            ry: u16,
            _: Option<&mut OverlayGrid>,
            _: Option<&OverlayTypeRegistry>,
            terrain: Option<&mut ResolvedTerrainGrid>,
            _: Option<&mut SimRng>,
            _: Option<&OccupancyGrid>,
        ) {
            if (rx, ry) == (5, 5) {
                self.visited = true;
                terrain
                    .unwrap()
                    .cell_mut_for_test(5, 5)
                    .unwrap()
                    .bridge_facts
                    .raw_flags = self.flags;
            }
        }
    }
    for initial_bridge in [false, true] {
        let (mut entities, occupancy, mut terrain, rules, warhead, interner) =
            bridge_layer_test_fixture();
        terrain
            .cell_mut_for_test(5, 5)
            .unwrap()
            .bridge_facts
            .raw_flags = if initial_bridge {
            BRIDGE_FLAG_STRUCTURAL
        } else {
            0
        };
        let mut prelude = ChangeBridge {
            flags: if initial_bridge {
                0
            } else {
                BRIDGE_FLAG_STRUCTURAL
            },
            visited: false,
        };
        let result = apply_aoe_damage_with_terrain_and_scenario(
            &mut entities,
            5,
            5,
            100,
            &warhead,
            &rules,
            &interner,
            None,
            "Americans",
            AoELayerContext {
                impact_z: 2,
                ..native_lists(&occupancy, &mut terrain, Some(209))
            },
            None,
            false,
            Some(&mut prelude),
        );
        assert!(prelude.visited);
        assert_eq!(
            result.object_list_layer,
            if initial_bridge {
                ListLayer::Bridge
            } else {
                ListLayer::Ground
            }
        );
        assert_eq!(
            hit_ids(&result.hits),
            vec![if initial_bridge { 2 } else { 1 }],
            "callbacks cannot reselect this call's list"
        );
    }
}

#[test]
fn bridge_object_layer_keeps_fixed_stride_alias_and_live_dummy_floor() {
    let (_, _, terrain, _, _, _) = bridge_layer_test_fixture();
    // (517,4) aliases the allocated native512-stride slot(5,5).
    assert_eq!(
        select_object_damage_layer(&terrain, 517, 4, center_impact(209)),
        ListLayer::Bridge
    );
    let dummy = terrain.shared_cell_dummy();
    dummy.test_set_retained_bridge_flags(BRIDGE_FLAG_STRUCTURAL);
    terrain.test_set_dummy_cell_level_slope(-1, 1);
    terrain.stamp_dummy_cell_requested_coord(77, 88);
    for (z_leptons, expected) in [(106, ListLayer::Ground), (107, ListLayer::Bridge)] {
        // Native signed levelFF ramp1 at local(1,0) is-102; Map578080
        // stamps this same retained Dummy and adds no deck to its floor.
        let impact = AoEAirImpact {
            sub_x: SimFixed::from_num(1),
            sub_y: SimFixed::ZERO,
            z_leptons,
        };
        assert_eq!(
            select_object_damage_layer(&terrain, 20, 20, impact),
            expected
        );
        assert_eq!(dummy.snapshot().coord, (20, 20));
    }
    assert_eq!(terrain.dummy_cell_level_slope(), (-1, 1));
    // Whole-Level compatibility uses the same canonical lookup/floor arm.
    let legacy = air_impact_from_layer_z(
        Some(&terrain),
        20,
        20,
        SimFixed::from_num(1),
        SimFixed::ZERO,
        1,
    )
    .unwrap();
    assert_eq!(legacy.z_leptons, 106);
    assert_eq!(
        select_object_damage_layer(&terrain, 20, 20, legacy),
        ListLayer::Ground
    );
}

// Stock house admission only. Ares replaces the ally gate with
// WarheadExt 10051B70 on the live attacker's owner (receiver_call.rs, with
// its own tests), where a same-owner target follows AffectsAllies.
#[test]
fn gsi_04_07_damage_receiver_pipeline_source_admission_and_house_snapshot() {
    fn run(
        damage_self: bool,
        warhead_name: &str,
    ) -> (
        Vec<u64>,
        Vec<Option<crate::sim::intern::InternedId>>,
        [i32; 3],
    ) {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=SOURCE\n1=TARGET\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=OrdinaryWH\n1=CrushWH\n2=SelfWH\n\
             [CombatDamage]\nCrushWarhead=CrushWH\nMaxDamage=10000\n\
             [SOURCE]\nStrength=100\nArmor=heavy\nDamageSelf={}\n\
             [TARGET]\nStrength=100\nArmor=heavy\n\
             [OrdinaryWH]\nCellSpread=0\nAffectsAllies=no\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [CrushWH]\nCellSpread=0\nAffectsAllies=no\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [SelfWH]\nCellSpread=0\nAffectsAllies=no\nAllowDamageOnSelf=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            if damage_self { "yes" } else { "no" }
        ));
        let rules = RuleSet::from_ini(&ini).expect("receiver admission fixture");
        let warhead = rules.warhead(warhead_name).unwrap().clone();
        let mut interner = test_interner();
        let _handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
        let warhead_ref = interner.intern(warhead_name);
        let source_house = interner.intern("SourceHouse");
        let ally_house = interner.intern("AllyHouse");
        let enemy_house = interner.intern("EnemyHouse");
        let source_type = interner.intern("SOURCE");
        let target_type = interner.intern("TARGET");

        let mut entities = EntityStore::new();
        let mut source = GameEntity::test_default(1, "SOURCE", "SourceHouse", 1, 1);
        source.set_type_ref_for_test(source_type);
        source.set_owner_for_test(source_house);
        entities.insert(source);
        let mut ally = GameEntity::test_default(2, "TARGET", "AllyHouse", 1, 1);
        ally.set_type_ref_for_test(target_type);
        ally.set_owner_for_test(ally_house);
        entities.insert(ally);
        let mut enemy = GameEntity::test_default(3, "TARGET", "EnemyHouse", 1, 1);
        enemy.set_type_ref_for_test(target_type);
        enemy.set_owner_for_test(enemy_house);
        entities.insert(enemy);
        let mut occupancy = OccupancyGrid::new();
        for stable_id in [3, 2, 1] {
            prepend_ground(&mut occupancy, 1, 1, stable_id);
        }
        let mut terrain = crate::map::resolved_terrain::test_grid(3, 3, test_terrain_cell);
        let aoe = apply_aoe_damage(
            &mut entities,
            1,
            1,
            40,
            &warhead,
            &rules,
            &mut interner,
            (1, Some(source_house), warhead_ref),
            native_lists(&occupancy, &mut terrain, None),
        );
        let ids = hit_ids(&aoe.hits);
        let captured_houses = aoe.hits.iter().map(|event| event.source_house).collect();

        // The event keeps the detonation's House, but stock AffectsAllies
        // (TechnoClass::ReceiveDamage 0x701CAC) asks the live attacker's
        // current House: after the source changes owner, EnemyHouse
        // answers, so the ally is hit and EnemyHouse's own units are not.
        entities.change_owner(1, enemy_house);
        let mut commit = ReceiverCommit::new();
        commit
            .alliances
            .entry("SOURCEHOUSE".to_string())
            .or_default()
            .insert("ALLYHOUSE".to_string());
        let (death, pings) = commit.run(
            &aoe.hits,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut terrain,
        );
        assert!(death.despawned_ids.is_empty());
        assert!(pings.is_empty());
        (
            ids,
            captured_houses,
            [
                entities.get(1).unwrap().health.current(),
                entities.get(2).unwrap().health.current(),
                entities.get(3).unwrap().health.current(),
            ],
        )
    }

    let source_house = test_intern("SourceHouse");
    let (ordinary_ids, ordinary_houses, ordinary_hp) = run(false, "OrdinaryWH");
    assert_eq!(ordinary_ids, vec![2, 3]);
    assert_eq!(ordinary_houses, vec![Some(source_house); 2]);
    assert_eq!(ordinary_hp, [100, 60, 100]);

    // The collected source is its own attacker, so its now-EnemyHouse
    // owner counts it an ally too.
    let (damage_self_ids, _, damage_self_hp) = run(true, "OrdinaryWH");
    assert_eq!(damage_self_ids, vec![1, 2, 3]);
    assert_eq!(damage_self_hp, [100, 60, 100]);

    let (crush_ids, _, crush_hp) = run(false, "CrushWH");
    assert_eq!(crush_ids, vec![1, 2, 3]);
    assert_eq!(crush_hp, [100, 60, 100]);

    // Phobos `Explosion_Damage_DamageSelf @ 0x004896EC` skips the source
    // exclusion, so a DamageSelf=no source still collects its own hit.
    #[cfg(feature = "phobos")]
    {
        let (allow_ids, _, allow_hp) = run(false, "SelfWH");
        assert_eq!(allow_ids, vec![1, 2, 3]);
        assert_eq!(allow_hp, [100, 60, 100]);
    }
}

#[test]
fn gsi_04_07_damage_object_immune_short_circuits_before_hp() {
    fn run(immune: bool) -> (i32, usize, crate::sim::combat::DeathEffects) {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=CABHUT\n\
             [Warheads]\n0=AP\n\
             [MTNK]\nStrength=300\nArmor=heavy\nPrimary=105mm\n\
             [CABHUT]\nStrength=2000\nArmor=none\nCanC4=yes\nImmune={}\n\
             [105mm]\nDamage=65\nWarhead=AP\n\
             [AP]\nCellSpread=0\nPercentAtMax=1\n\
             Verses=25%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            if immune { "yes" } else { "no" }
        ));
        let rules = RuleSet::from_ini(&ini).expect("CABHUT Immune fixture");
        let warhead = rules.warhead("AP").unwrap().clone();
        assert_eq!(rules.object("CABHUT").unwrap().immune, immune);
        assert_eq!(rules.weapon("105mm").unwrap().damage, 65);

        let mut interner = test_interner();
        let soviet = interner.intern("SovietHouse");
        let civilian = interner.intern("CivilianHouse");
        let mtnk = interner.intern("MTNK");
        let cabhut = interner.intern("CABHUT");
        let ap = interner.intern("AP");
        let mut entities = EntityStore::new();
        let mut source = GameEntity::test_default(1, "MTNK", "SovietHouse", 1, 1);
        source.set_owner_for_test(soviet);
        source.set_type_ref_for_test(mtnk);
        entities.insert(source);
        let mut hut = GameEntity::test_default_of_category(
            2,
            "CABHUT",
            "CivilianHouse",
            5,
            5,
            EntityCategory::Structure,
        );
        hut.set_owner_for_test(civilian);
        hut.set_type_ref_for_test(cabhut);
        hut.health.set_for_test(2000);
        entities.insert(hut);

        let mut occupancy = OccupancyGrid::new();
        append_building(&mut occupancy, 5, 5, 2);
        let mut terrain = crate::map::resolved_terrain::test_grid(8, 8, test_terrain_cell);
        let aoe = apply_aoe_damage(
            &mut entities,
            5,
            5,
            65,
            &warhead,
            &rules,
            &mut interner,
            (1, Some(soviet), ap),
            native_lists(&occupancy, &mut terrain, None),
        );
        assert_eq!(
            aoe.hits
                .iter()
                .map(|event| (event.target_id, event.damage, event.distance_leptons))
                .collect::<Vec<_>>(),
            vec![(2, 65, 0)],
            "Immune remains an ordered receiver record, not a collector filter"
        );

        let mut commit = ReceiverCommit {
            sounds: Some(Vec::new()),
            ..ReceiverCommit::new()
        };
        let (death, pings) = commit.run(
            &aoe.hits,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut terrain,
        );
        assert!(pings.is_empty(), "building warnings are already submitted");
        let warnings = commit
            .sounds
            .unwrap()
            .iter()
            .filter(|event| matches!(event, crate::sim::world::SimSoundEvent::UnderAttack { .. }))
            .count();
        (entities.get(2).unwrap().health.current(), warnings, death)
    }

    let (immune_hp, immune_pings, immune_death) = run(true);
    assert_eq!(immune_hp, 2000);
    assert_eq!(immune_pings, 0);
    assert!(immune_death.despawned_ids.is_empty());
    assert!(immune_death.immediate_uninit_ids.is_empty());
    assert!(immune_death.explosion_effects.is_empty());

    let (ordinary_hp, ordinary_pings, ordinary_death) = run(false);
    assert_eq!(ordinary_hp, 1984, "ftol(65 x AP none 25%) = 16");
    assert_eq!(ordinary_pings, 1, "ordinary enemy structure hit pings");
    assert!(ordinary_death.despawned_ids.is_empty());
}

#[test]
fn gsi_04_07_damage_live_defender_stronger_uses_veteran_armor() {
    let ini = IniFile::from_str(
        "[General]\nVeteranArmor=1.5\n\
         [Normal]\nArmor=1.0\n\
         [Countries]\n0=Americans\n1=Russians\n\
         [Americans]\nArmor=1.0\nArmorUnitsMult=1.0\n\
         [Russians]\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=HTNK\n1=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [HTNK]\nStrength=400\nArmor=heavy\nPrimary=RhinoGun\n\
         [MTNK]\nStrength=300\nArmor=heavy\nVeteranAbilities=STRONGER\nEliteAbilities=STRONGER\n\
         [RhinoGun]\nDamage=90\nWarhead=AP\n\
         [AP]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("Rhino/Grizzly receiver fixture");
    let warhead = rules.warhead("AP").expect("AP").clone();
    assert_eq!(rules.weapon("RhinoGun").unwrap().damage, 90);
    assert!(
        rules
            .object("MTNK")
            .unwrap()
            .veteran_abilities
            .has(crate::rules::object_type::Ability::Stronger)
    );
    assert_eq!(rules.general.veteran_armor, 1.5);

    let mut interner = test_interner();
    let russian = interner.intern("Russians");
    let american = interner.intern("Americans");
    let htnk = interner.intern("HTNK");
    let mtnk = interner.intern("MTNK");
    let ap = interner.intern("AP");
    let mut entities = EntityStore::new();
    let mut source = GameEntity::test_default(1, "HTNK", "Russians", 1, 1);
    source.set_owner_for_test(russian);
    source.set_type_ref_for_test(htnk);
    entities.insert(source);
    let mut rookie = GameEntity::test_default(2, "MTNK", "Americans", 5, 5);
    rookie.set_owner_for_test(american);
    rookie.set_type_ref_for_test(mtnk);
    rookie.health.set_for_test(300);
    entities.insert(rookie);
    let mut veteran = GameEntity::test_default(3, "MTNK", "Americans", 5, 5);
    veteran.set_owner_for_test(american);
    veteran.set_type_ref_for_test(mtnk);
    veteran.health.set_for_test(300);
    veteran.veterancy.set_rank_for_test(100);
    entities.insert(veteran);

    let mut occupancy = OccupancyGrid::new();
    for stable_id in [3, 2] {
        prepend_ground(&mut occupancy, 5, 5, stable_id);
    }
    let mut terrain = crate::map::resolved_terrain::test_grid(8, 8, test_terrain_cell);
    let aoe = apply_aoe_damage(
        &mut entities,
        5,
        5,
        90,
        &warhead,
        &rules,
        &mut interner,
        (1, Some(russian), ap),
        native_lists(&occupancy, &mut terrain, None),
    );
    assert_eq!(
        aoe.hits
            .iter()
            .map(|event| (event.target_id, event.damage, event.distance_leptons))
            .collect::<Vec<_>>(),
        vec![(2, 90, 0), (3, 90, 0)],
        "Apply_area_damage records keep raw Rhino damage for the ordered receiver"
    );

    let mut commit = ReceiverCommit::new();
    let mut target_house = HouseState::new(american, 0, Some(american), true, 0, 10);
    target_house.set_difficulty_for_test(HouseDifficulty::Normal);
    commit.houses.register(target_house);
    let (death, pings) = commit.run(
        &aoe.hits,
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut terrain,
    );

    assert_eq!(entities.get(2).unwrap().health.current(), 210);
    assert_eq!(
        entities.get(3).unwrap().health.current(),
        240,
        "STRONGER divides 90 by stock VeteranArmor 1.5 at the live receiver"
    );
    assert!(death.despawned_ids.is_empty());
    assert!(pings.is_empty());
}

#[test]
fn gsi_04_07_damage_psychgas_commits_berserk_transaction() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=CAOS\n1=TARGET\n2=IMMUNE\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=BUILD\n\
         [Warheads]\n0=PsychGasCreate\n1=PsychGas\n\
         [CAOS]\nStrength=100\nArmor=heavy\nPrimary=ChaosAttack\n\
         [TARGET]\nStrength=1000\nArmor=heavy\nImmuneToPsionics=no\n\
         [IMMUNE]\nStrength=1000\nArmor=heavy\nImmuneToPsionics=yes\n\
         [BUILD]\nStrength=1000\nArmor=heavy\nImmuneToPsionics=no\n\
         [ChaosAttack]\nDamage=600\nWarhead=PsychGasCreate\n\
         [PsychGasCreate]\nCellSpread=3\nPercentAtMax=1\nPsychedelic=yes\nAffectsAllies=yes\n\
         Verses=100%,100%,100%,100%,100%,50%,100%,100%,100%,100%,100%\n\
         [PsychGas]\nCellSpread=3\nPercentAtMax=1\nPsychedelic=yes\nAffectsAllies=yes\n\
         Verses=100%,100%,100%,100%,100%,50%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("stock-like Chaos gas fixture");
    let warhead = rules.warhead("PsychGasCreate").unwrap().clone();
    assert_eq!(rules.weapon("ChaosAttack").unwrap().damage, 600);
    let armor = rules.object("TARGET").unwrap().armor_id();
    assert_eq!(
        f64::from_bits(warhead.interaction(armor).multiplier().bits()),
        0.5
    );
    assert_eq!(
        f64::from_bits(
            rules
                .warhead("PsychGas")
                .unwrap()
                .interaction(armor)
                .multiplier()
                .bits()
        ),
        0.5
    );

    let mut interner = test_interner();
    let yuri = interner.intern("YuriHouse");
    let soviet = interner.intern("SovietHouse");
    let caos = interner.intern("CAOS");
    let target_type = interner.intern("TARGET");
    let immune_type = interner.intern("IMMUNE");
    let building_type = interner.intern("BUILD");
    let psych_gas_create = interner.intern("PsychGasCreate");

    let mut entities = EntityStore::new();
    let mut source = GameEntity::test_default(1, "CAOS", "YuriHouse", 1, 1);
    source.set_owner_for_test(yuri);
    source.set_type_ref_for_test(caos);
    entities.insert(source);

    let mut target = GameEntity::test_default(2, "TARGET", "SovietHouse", 5, 5);
    target.set_owner_for_test(soviet);
    target.set_type_ref_for_test(target_type);
    target.health.set_for_test(1000);
    target
        .attack_target
        .set_for_test(Some(TargetKind::Entity(1)));
    entities.insert(target);

    let mut allied = GameEntity::test_default(3, "TARGET", "YuriHouse", 5, 5);
    allied.set_owner_for_test(yuri);
    allied.set_type_ref_for_test(target_type);
    allied.health.set_for_test(1000);
    allied
        .attack_target
        .set_for_test(Some(TargetKind::Entity(1)));
    entities.insert(allied);

    let mut immune = GameEntity::test_default(4, "IMMUNE", "SovietHouse", 5, 5);
    immune.set_owner_for_test(soviet);
    immune.set_type_ref_for_test(immune_type);
    immune.health.set_for_test(1000);
    immune
        .attack_target
        .set_for_test(Some(TargetKind::Entity(1)));
    entities.insert(immune);

    let mut building = GameEntity::test_default_of_category(
        5,
        "BUILD",
        "SovietHouse",
        5,
        5,
        EntityCategory::Structure,
    );
    building.set_owner_for_test(soviet);
    building.set_type_ref_for_test(building_type);
    building.health.set_for_test(1000);
    building
        .attack_target
        .set_for_test(Some(TargetKind::Entity(1)));
    entities.insert(building);

    let mut occupancy = OccupancyGrid::new();
    for stable_id in [4, 3, 2] {
        prepend_ground(&mut occupancy, 5, 5, stable_id);
    }
    append_building(&mut occupancy, 5, 5, 5);
    let mut terrain = crate::map::resolved_terrain::test_grid(12, 12, test_terrain_cell);
    let aoe = apply_aoe_damage(
        &mut entities,
        5,
        5,
        rules.weapon("ChaosAttack").unwrap().damage,
        &warhead,
        &rules,
        &mut interner,
        (1, Some(yuri), psych_gas_create),
        native_lists(&occupancy, &mut terrain, None),
    );
    assert_eq!(
        aoe.hits
            .iter()
            .map(|event| (event.target_id, event.damage, event.distance_leptons))
            .collect::<Vec<_>>(),
        vec![(2, 600, 0), (3, 600, 0), (4, 600, 0), (5, 600, 0)],
        "Apply_area_damage preserves raw ordered records; Psychedelic gates live in the receiver"
    );

    let mut commit = ReceiverCommit::new();
    let (death, pings) = commit.run(
        &aoe.hits,
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut terrain,
    );
    assert!(death.despawned_ids.is_empty());
    assert!(pings.is_empty());

    let target = entities.get(2).unwrap();
    assert_eq!(target.health.current(), 1000, "Psychedelic skips Object HP");
    assert!(target.berserk.is_active());
    assert_eq!(target.berserk.timer(), 300, "600 x heavy Verses 50%");
    assert!(target.attack_target.target().is_none());
    assert_eq!(target.mission.current(), MissionId::NONE);
    assert_eq!(
        target.mission.queued(),
        MissionId::from_known(MissionType::Hunt)
    );

    for stable_id in [3, 4, 5] {
        let control = entities.get(stable_id).unwrap();
        assert_eq!(control.health.current(), 1000);
        assert_eq!(control.berserk, Default::default());
        assert_eq!(control.attack_target.target(), Some(TargetKind::Entity(1)));
        assert_eq!(control.mission.queued(), MissionId::NONE);
    }

    // A later gas record refreshes only +0x29C: it does not repeat target
    // clearing or mission queue callbacks while +0x298 remains active.
    {
        let target = entities.get_mut(2).unwrap();
        target
            .attack_target
            .set_for_test(Some(TargetKind::Entity(1)));
        target.berserk.set_timer_for_test(17);
        target.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Guard),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
    }
    let (refresh_death, refresh_pings) = commit.run(
        &aoe.hits[..1],
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut terrain,
    );
    let target = entities.get(2).unwrap();
    assert!(refresh_death.despawned_ids.is_empty());
    assert!(refresh_pings.is_empty());
    assert_eq!(target.health.current(), 1000);
    assert_eq!(target.berserk.timer(), 300);
    assert_eq!(target.attack_target.target(), Some(TargetKind::Entity(1)));
    assert_eq!(target.mission.queued(), MissionId::NONE);
}

/// A rocket's DamageArea (`0x006632BB`) passes a live source and a NULL
/// house; DamageArea forwards that NULL (`0x00489AA7`), so the Psychedelic
/// gate (`0x00701CE5`) finds no ally and the firer's own unit goes berserk.
#[test]
fn area_damage_without_source_house_berserks_the_firers_units() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=FIRER\n1=TARGET\n[AircraftTypes]\n\
         [BuildingTypes]\n[Warheads]\n0=PsyWH\n\
         [FIRER]\nStrength=100\nArmor=heavy\n\
         [TARGET]\nStrength=1000\nArmor=heavy\nImmuneToPsionics=no\n\
         [PsyWH]\nCellSpread=1\nPercentAtMax=1\nPsychedelic=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("psychedelic rocket fixture");
    let warhead = rules.warhead("PsyWH").unwrap().clone();
    let mut interner = test_interner();
    let yuri = interner.intern("YuriHouse");
    let warhead_ref = interner.intern("PsyWH");
    let mut entities = EntityStore::new();
    for (id, type_name, rx) in [(1, "FIRER", 1), (2, "TARGET", 5)] {
        let mut unit = GameEntity::test_default(id, type_name, "YuriHouse", rx, 5);
        unit.set_owner_for_test(yuri);
        unit.set_type_ref_for_test(interner.intern(type_name));
        entities.insert(unit);
    }
    let mut occupancy = OccupancyGrid::new();
    prepend_ground(&mut occupancy, 5, 5, 2);
    let mut terrain = crate::map::resolved_terrain::test_grid(12, 12, test_terrain_cell);
    let aoe = apply_aoe_damage(
        &mut entities,
        5,
        5,
        200,
        &warhead,
        &rules,
        &mut interner,
        (1, None, warhead_ref),
        native_lists(&occupancy, &mut terrain, None),
    );
    let mut commit = ReceiverCommit::new();
    commit.run(
        &aoe.hits,
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut terrain,
    );
    assert!(entities.get(2).unwrap().berserk.is_active());
}

#[test]
fn gsi_04_07_damage_prone_raw_scaling_precedes_area_receiver_pipeline() {
    let ini = IniFile::from_str(
        "[General]\nFixtureOnly=1\n\
         [Normal]\nArmor=1.0\n\
         [Easy]\nArmor=1.2\n\
         [Countries]\n0=EasyHouse\n\
         [EasyHouse]\nArmorInfantryMult=1.2\n\
         [InfantryTypes]\n0=E1\n\
         [VehicleTypes]\n0=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [MTNK]\nStrength=300\nArmor=heavy\nPrimary=105mmE\n\
         [E1]\nStrength=100\nArmor=none\n\
         [105mmE]\nDamage=65\nWarhead=GRIZAPE\n\
         [GRIZAPE]\nCellSpread=.3\nPercentAtMax=.5\nProneDamage=50%\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("105mmE/GRIZAPE prone receiver fixture");
    let warhead = rules.warhead("GRIZAPE").expect("GRIZAPE").clone();
    assert_eq!(rules.weapon("105mmE").unwrap().damage, 65);
    assert_eq!(warhead.prone_damage_f64, 0.5);

    let mut interner = test_interner();
    let source_house = interner.intern("SourceHouse");
    let normal_house = interner.intern("NormalHouse");
    let easy_house = interner.intern("EasyHouse");
    let mtnk = interner.intern("MTNK");
    let e1 = interner.intern("E1");
    let grizape = interner.intern("GRIZAPE");
    let mut entities = EntityStore::new();
    let mut source = GameEntity::test_default(1, "MTNK", "SourceHouse", 1, 1);
    source.set_owner_for_test(source_house);
    source.set_type_ref_for_test(mtnk);
    entities.insert(source);

    let mut target = GameEntity::test_default(2, "E1", "NormalHouse", 5, 5);
    target.set_owner_for_test(normal_house);
    target.set_type_ref_for_test(e1);
    target.set_category_for_test(EntityCategory::Infantry);
    target.health.set_for_test(20);
    target.infantry = Some(crate::sim::infantry::InfantryRuntime::new());
    target
        .infantry
        .as_mut()
        .unwrap()
        .set_is_prone_for_test(true);
    entities.insert(target);

    let mut easy_prone = GameEntity::test_default(3, "E1", "EasyHouse", 5, 5);
    easy_prone.set_owner_for_test(easy_house);
    easy_prone.set_type_ref_for_test(e1);
    easy_prone.set_category_for_test(EntityCategory::Infantry);
    easy_prone.health.set_for_test(100);
    easy_prone.infantry = Some(crate::sim::infantry::InfantryRuntime::new());
    easy_prone
        .infantry
        .as_mut()
        .unwrap()
        .set_is_prone_for_test(true);
    entities.insert(easy_prone);

    let mut easy_standing = GameEntity::test_default(4, "E1", "EasyHouse", 5, 5);
    easy_standing.set_owner_for_test(easy_house);
    easy_standing.set_type_ref_for_test(e1);
    easy_standing.set_category_for_test(EntityCategory::Infantry);
    easy_standing.health.set_for_test(100);
    easy_standing.infantry = Some(crate::sim::infantry::InfantryRuntime::new());
    entities.insert(easy_standing);

    let mut occupancy = OccupancyGrid::new();
    for stable_id in [4, 3, 2] {
        prepend_ground(&mut occupancy, 5, 5, stable_id);
    }
    let mut terrain = crate::map::resolved_terrain::test_grid(8, 8, test_terrain_cell);
    let aoe = apply_aoe_damage(
        &mut entities,
        5,
        5,
        65,
        &warhead,
        &rules,
        &mut interner,
        (1, Some(source_house), grizape),
        native_lists(&occupancy, &mut terrain, None),
    );
    assert_eq!(
        aoe.hits
            .iter()
            .map(|event| (event.target_id, event.damage, event.distance_leptons))
            .collect::<Vec<_>>(),
        vec![(2, 65, 0), (3, 65, 0), (4, 65, 0)],
        "collection preserves stock 105mmE raw records"
    );

    let mut commit = ReceiverCommit::new();
    let mut normal = HouseState::new(normal_house, 0, None, false, 0, 10);
    normal.set_difficulty_for_test(HouseDifficulty::Normal);
    commit.houses.register(normal);
    let mut easy = HouseState::new(easy_house, 0, None, false, 0, 10);
    easy.set_difficulty_for_test(HouseDifficulty::Easy);
    commit.houses.register(easy);
    let (death, _) = commit.run(
        &aoe.hits,
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut terrain,
    );

    assert_eq!(entities.get(2).unwrap().health.current(), 0);
    assert!(death.despawned_ids.contains(&2));
    // The Easy house's `[Easy] Armor=1.2` divides nothing: it only feeds
    // `House+0x1A0`, which no damage path reads. Its country's
    // `ArmorInfantryMult=1.2` does (`GetArmorMultForType @ 0x0050BD30`),
    // after the prone pre-scale: ftol(ftol(65 * .5) / 1.2) = 26. Dividing
    // first would give ftol(ftol(65 / 1.2) * .5) = 27.
    assert_eq!(
        entities.get(3).unwrap().health.current(),
        74,
        "ftol(65 * .5) = 32, then the country's 1.2 yields 26 (not 27)"
    );
    assert_eq!(
        entities.get(4).unwrap().health.current(),
        46,
        "standing Infantry bypasses the concrete prone pre-scale: 65 / 1.2 = 54"
    );
}

#[test]
fn gsi_04_07_damage_nonfatal_infantry_scatter_precedes_fear() {
    #[derive(Debug)]
    struct Outcome {
        health: i32,
        fear: u16,
        destination: Option<(u16, u16)>,
        queued_mission: MissionId,
        rng_changed: bool,
        rng_indices: serde_json::Value,
        setter: serde_json::Value,
    }

    #[derive(Default)]
    struct ScatterFixture {
        doing: i32,
        animation: Option<crate::sim::animation::SequenceKind>,
        first_bridge: bool,
        all_bridges: bool,
        head: Option<crate::sim::components::DriveCoord>,
        outside: bool,
        entry_case: Option<serde_json::Value>,
    }

    fn run(
        mission: MissionType,
        attacker_present: bool,
        health: i32,
        fraidycat: bool,
        human_control: Option<bool>,
        team: bool,
        nav: bool,
    ) -> Outcome {
        run_fixture(
            mission,
            attacker_present,
            health,
            fraidycat,
            human_control,
            team,
            nav,
            ScatterFixture {
                doing: -1,
                ..Default::default()
            },
        )
    }

    fn run_fixture(
        mission: MissionType,
        attacker_present: bool,
        health: i32,
        fraidycat: bool,
        human_control: Option<bool>,
        team: bool,
        nav: bool,
        fixture: ScatterFixture,
    ) -> Outcome {
        let ini = IniFile::from_str(&format!(
            "[General]\nFixture=1\n[InfantryTypes]\n0=E1\n\
             [VehicleTypes]\n0=ATTACKER\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=WH\n\
             [IQ]\nScatter=99\n\
             [CombatDamage]\nPlayerScatter=no\nMaxDamage=10000\n\
             [Guard]\nScatter=yes\n[Enter]\nScatter=yes\n\
             [Attack]\nScatter=no\n\
             [E1]\nStrength=125\nArmor=none\nSpeed=4\nFraidycat={fraidycat}\n\
             Locomotor={{4A582744-9839-11d1-B709-00A024DDAFD1}}\n\
             MovementZone=Infantry\nSpeedType=Foot\n\
             [ATTACKER]\nStrength=100\nArmor=none\nOpenTopped=yes\n\
             [WH]\nCellSpread=0\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ));
        let mut rules = RuleSet::from_ini(&ini).expect("damage-scatter fixture");
        rules.general.blockage_path_delay_ticks = 22;
        let mut interner = test_interner();
        let attacker_house = interner.intern("AttackerHouse");
        let victim_house = interner.intern("VictimHouse");
        let attacker_type = interner.intern("ATTACKER");
        let victim_type = interner.intern("E1");
        let warhead_ref = interner.intern("WH");

        let mut entities = EntityStore::new();
        let mut attacker = GameEntity::test_default(1, "ATTACKER", "AttackerHouse", 3, 5);
        attacker.set_owner_for_test(attacker_house);
        attacker.set_type_ref_for_test(attacker_type);
        entities.insert(attacker);

        let mut victim = GameEntity::test_default(2, "E1", "VictimHouse", 5, 5);
        victim.set_owner_for_test(victim_house);
        victim.set_type_ref_for_test(victim_type);
        victim.set_category_for_test(EntityCategory::Infantry);
        victim.mission_leaf = crate::sim::mission::leaf::MissionLeafState::for_entity_category(
            EntityCategory::Infantry,
        );
        victim
            .mission_leaf
            .set_infantry_doing_verified(fixture.doing)
            .unwrap();
        victim.animation = fixture.animation.map(crate::sim::animation::Animation::new);
        victim.health.set_for_test(health);
        victim.set_sub_cell(Some(2));
        victim.set_on_bridge(
            fixture
                .entry_case
                .as_ref()
                .and_then(|case| case["on_bridge"].as_bool())
                .unwrap_or(false),
        );
        victim.infantry = Some(crate::sim::infantry::InfantryRuntime::new());
        if nav {
            victim
                .navigation
                .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::cell(8, 8)));
        }
        victim.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::from_object_type(
                rules.object("E1").expect("E1 type"),
                0,
            ),
        );
        if let Some(head) = fixture.head {
            victim.locomotor.as_mut().unwrap().set_step_head(Some(head));
        }
        victim.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(mission),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        if let Some(case) = fixture
            .entry_case
            .as_ref()
            .filter(|c| c["live_setter"] == true)
        {
            use crate::sim::components::{DriveCoord, FootPathQueue};
            use crate::sim::timer::CdTimer;
            victim.body_facing.snap(0x4000, 0);
            victim
                .infantry
                .as_mut()
                .unwrap()
                .set_cell_entry_blocked_for_test(true);
            victim
                .navigation
                .set_nav_com_aux_for_test(Some(crate::sim::components::NavTargetRef::cell(1, 1)));
            victim
                .navigation
                .set_path_replay_for_test(FootPathQueue::for_test(
                    vec![2, 3, 4, 5],
                    0,
                    Some((9, 8)),
                ));
            let path = victim.navigation.path_runtime_mut();
            path.set_movement_timer_for_test(CdTimer::started(50, 5));
            path.set_blocked_timer_for_test(CdTimer::started(40, 6));
            path.set_path_blocked_for_test(true);
            path.set_retries_left_for_test(7);
            let loco = victim.locomotor.as_mut().unwrap();
            loco.set_powered_for_test(case["power_off"] != true);
            if let Some(head) = case["head"].as_array() {
                loco.set_step_head(Some(DriveCoord {
                    x: head[0].as_i64().unwrap() as i32 - 1280,
                    y: head[1].as_i64().unwrap() as i32 - 1280,
                    z: head[2].as_i64().unwrap() as i32,
                }));
            }
            if case["moving"] == true {
                loco.set_walk_destination(Some(DriveCoord {
                    x: 6528,
                    y: 1408,
                    z: 0,
                }));
            }
            victim
                .lift
                .set_swap_active_for_test(case["swap_active"] == true);
            if case["bunker"] == true {
                victim.bunker.set_bunker_link_for_test(
                    crate::sim::docking::bunker_link::BunkerLink::Installed(1),
                );
            }
            // BeingWarpedOut (`+0x270`) through a Temporal chain's
            // head, WarpingIn (`+0x271`) through the teleport's warp-in.
            if case["warp_out"] == true {
                victim.set_temporal_for_test(
                    crate::sim::temporal::TemporalState::warped_by_for_test(99),
                );
            }
            if case["warp_in"] == true {
                use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
                victim.install_teleport_state_for_test(Some(TeleportState::for_test(
                    TeleportPhase::ChronoDelay,
                    8,
                    8,
                    3,
                )));
            }
            if case["open_transport"] == true {
                victim.set_passenger_role_for_test(
                    crate::sim::passenger::PassengerRole::inside_for_test(1, true),
                );
            }
        }
        entities.insert(victim);

        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            5,
            5,
            2,
            MovementLayer::Ground,
            Some(2),
            CellListInsertion::PrependNonBuilding,
        );
        let cells = (0..10)
            .flat_map(|ry| {
                (0..10).map(move |rx| {
                    let mut cell = test_terrain_cell(rx, ry);
                    cell.speed_costs.foot = Some(crate::util::native_x87::NativeF32Bits::ONE);
                    cell
                })
            })
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells(10, 10, cells);
        if fixture.first_bridge {
            terrain
                .cell_mut_for_test(6, 4)
                .unwrap()
                .bridge_facts
                .raw_flags = 0x100;
        }
        if fixture.all_bridges {
            for (dx, dy) in crate::util::direction::DIRECTION_DELTAS {
                terrain
                    .cell_mut_for_test((5 + dx) as u16, (5 + dy) as u16)
                    .unwrap()
                    .bridge_facts
                    .raw_flags = 0x100;
            }
        }
        let mut raw = crate::sim::occupancy::RawCellOccupationGrid::new();
        if let Some(case) = fixture.entry_case.as_ref() {
            // Translate the native witness from (10,10) to this damage
            // receiver fixture's (5,5); source heading and neighbours agree.
            let xy = |v: &serde_json::Value| {
                (
                    (v[0].as_u64().unwrap() - 5) as u16,
                    (v[1].as_u64().unwrap() - 5) as u16,
                )
            };
            for cell in case["cells"].as_array().into_iter().flatten() {
                let (x, y) = xy(cell);
                let out = terrain.cell_mut_for_test(x, y).unwrap();
                out.level = cell[2].as_i64().unwrap() as u8;
                out.bridge_facts.raw_flags = cell[3].as_u64().unwrap() as u32;
            }
            for cell in case["slopes"].as_array().into_iter().flatten() {
                let (x, y) = xy(cell);
                terrain.cell_mut_for_test(x, y).unwrap().slope_type =
                    cell[2].as_u64().unwrap() as u8;
            }
            for cell in case["blocked_terrain"].as_array().into_iter().flatten() {
                let (x, y) = xy(cell);
                terrain.cell_mut_for_test(x, y).unwrap().speed_costs.foot =
                    Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO);
            }
            for cell in case["raw"].as_array().into_iter().flatten() {
                let (x, y) = xy(cell);
                raw.mark_ground(x, y, cell[2].as_u64().unwrap() as u8);
                raw.mark_deck(x, y, cell[3].as_u64().unwrap() as u8);
            }
        }
        let mut event = EntityDamageEvent::area(
            2,
            10,
            0,
            if attacker_present {
                1
            } else {
                crate::sim::combat::RAD_NO_ATTACKER
            },
            attacker_present.then_some(attacker_house),
            warhead_ref,
        );
        if fixture
            .entry_case
            .as_ref()
            .is_some_and(|c| c["warp_out"] == true || c["bunker"] == true)
        {
            // Ordinary damage is immune before Scatter in these states.
            // Explicit ignore-defenses input reaches the same production
            // receiver continuation so the native setter refusal is tested.
            event.receiver_flags.ignore_defenses = true;
        }
        let mut world = crate::sim::world::Simulation::new();
        world.substrate.entities = entities;
        world.substrate.occupancy = occupancy;
        world.substrate.raw_cell_occupation = raw;
        world.interner = interner;
        world.resolved_terrain = Some(terrain);
        world.set_playfield_bounds_for_test(Some(
            crate::sim::cell_rect::PlayfieldBounds::from_normalized_local_size(
                16, -16, -16, 64, 64,
            ),
        ));
        if fixture.outside {
            world.set_playfield_bounds_for_test(Some(
                crate::sim::cell_rect::PlayfieldBounds::from_normalized_local_size(16, 0, 0, 1, 1),
            ));
        }
        world.session.set_binary_frame_for_test(100);
        world.main_rng = SimRng::new(7);
        world.scenario_rng = SimRng::new(1);
        // Some(false) models campaign PlayerControl without IsHuman.
        world.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false),
        );
        let mut house = HouseState::new(victim_house, 0, None, human_control == Some(true), 0, 10);
        house.set_player_control_for_test(human_control.is_some());
        world.houses.register(house);
        if team {
            let script = world.interner.intern("ScatterFixture");
            world
                .team_script_vm
                .create_team(victim_house, script, vec![2], 0);
        }
        let before_rng = world.scenario_rng.state();
        let mut receiver = crate::sim::combat::world_receiver::ReceiverRun::default();
        crate::sim::combat::world_receiver::commit_entities(
            &mut world,
            &mut receiver,
            std::slice::from_ref(&event),
            None,
            &rules,
            None,
            &mut Vec::new(),
        );
        let victim = world
            .substrate
            .entities
            .get(2)
            .expect("victim remains represented");
        Outcome {
            health: victim.health.current(),
            fear: victim
                .infantry
                .as_ref()
                .expect("Infantry runtime")
                .fear_level(),
            destination: victim
                .locomotor
                .as_ref()
                .and_then(|l| l.walk_destination_cell()),
            queued_mission: victim.mission.queued(),
            rng_changed: world.scenario_rng.state() != before_rng,
            rng_indices: {
                let state = world.scenario_rng.logical_state();
                serde_json::json!([state.index_a, state.index_b])
            },
            setter: {
                let loco = victim.locomotor.as_ref().unwrap();
                let path = victim.navigation.path_runtime();
                let coord = |value: Option<crate::sim::components::DriveCoord>| {
                    value.map_or([0, 0, 0], |v| [v.x + 1280, v.y + 1280, v.z])
                };
                let nav = victim.navigation.nav_com().and_then(|target| match target {
                    crate::sim::components::NavTargetRef::Cell { rx, ry } => Some([rx + 5, ry + 5]),
                    _ => None,
                });
                serde_json::json!({
                    "nav": nav, "aux": if victim.navigation.nav_com_aux().is_none() { 0 } else { 123 },
                    "destination": coord(loco.walk_destination()), "head": coord(loco.step_head()),
                    "queue": victim.navigation.path_replay().directions().iter().map(|&v| if v == 255 { -1 } else { i32::from(v) }).collect::<Vec<_>>(),
                    "reference": victim.navigation.path_replay().reference_cell(),
                    "queued_mission": victim.mission.queued().raw(),
                    "facing": u32::from(victim.body_facing.destination()) * 65537,
                    "moving": u8::from(loco.walk_is_moving().unwrap()), "powered": u8::from(loco.is_powered()),
                    "blocked": u8::from(path.path_blocked()),
                    "movement_timer": [path.movement_timer().start_frame(), path.movement_timer().duration()],
                    "blocked_timer": [path.blocked_timer().start_frame(), path.blocked_timer().duration()],
                    "retries": path.retries_left(),
                    "entry_blocked": u8::from(victim.infantry.as_ref().unwrap().cell_entry_blocked()),
                })
            },
        }
    }

    // A human's Fraidycat in a Team: TechnoClass::ReceiveDamage's own
    // scatter tail refuses him (no PlayerScatter= or SCATTER,
    // `0x00702C8A..0x00702CBC`) and the Team report returns at once for an
    // unarmed head, so the source Scatter these witnesses execute is the
    // receiver's only one. The Team admits it (`0x0051D200`); a computer's
    // man would take the tail's null Scatter first.
    let run_scene = |fixture| {
        run_fixture(
            MissionType::Guard,
            true,
            125,
            true,
            Some(true),
            true,
            false,
            fixture,
        )
    };
    let fixture = || ScatterFixture {
        doing: -1,
        ..Default::default()
    };
    // These witnesses execute original Scatter51D0D0 and Infantry51BF90
    // together. Exercise the production damage receiver, including HP,
    // queue/destination writes and fear, with the same entry prestates.
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_scatter_entry.json",
    ))
    .unwrap();
    assert_eq!(corpus.as_array().unwrap().len(), 14);
    for row in corpus.as_array().unwrap() {
        let output = run_scene(ScatterFixture {
            entry_case: Some(row["input"].clone()),
            ..fixture()
        });
        let expected = row["destination"].as_array().map(|xy| {
            (
                (xy[0].as_u64().unwrap() - 5) as u16,
                (xy[1].as_u64().unwrap() - 5) as u16,
            )
        });
        assert_eq!(output.destination, expected, "{}", row["input"]);
        assert_eq!(
            output.rng_indices, row["random_indices"],
            "{}",
            row["input"]
        );
        assert_eq!(output.health, 115);
        assert_eq!(output.fear, 300);
        assert_eq!(
            output.queued_mission,
            expected.map_or(MissionId::NONE, |_| {
                MissionId::from_known(MissionType::Move)
            })
        );
    }
    let destinations: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_scatter_destination.json",
    ))
    .unwrap();
    assert_eq!(destinations.as_array().unwrap().len(), 18);
    for row in destinations.as_array().unwrap() {
        let mission = if row["input"]["mission"] == 7 {
            MissionType::Enter
        } else {
            MissionType::Guard
        };
        let output = run_fixture(
            mission,
            true,
            125,
            true,
            Some(true),
            true,
            false,
            ScatterFixture {
                entry_case: Some(row["input"].clone()),
                ..fixture()
            },
        );
        assert_eq!(output.setter, row["setter"], "{}", row["input"]);
        assert_eq!(
            output.rng_indices, row["random_indices"],
            "{}",
            row["input"]
        );
        assert_eq!((output.health, output.fear), (115, 300));
    }
    let displayed_death = run_scene(ScatterFixture {
        animation: Some(crate::sim::animation::SequenceKind::Die1),
        ..fixture()
    });
    assert_eq!(
        displayed_death.destination,
        Some((6, 4)),
        "display does not own Doing"
    );
    let refused_doing = run_scene(ScatterFixture {
        doing: 7,
        animation: Some(crate::sim::animation::SequenceKind::Stand),
        ..fixture()
    });
    assert_eq!(refused_doing.destination, None);
    assert!(!refused_doing.rng_changed);
    let later_preferred = run_scene(ScatterFixture {
        first_bridge: true,
        ..fixture()
    });
    assert_eq!(later_preferred.destination, Some((6, 5)));
    let fallback = run_scene(ScatterFixture {
        all_bridges: true,
        ..fixture()
    });
    assert_eq!(fallback.destination, Some((6, 4)));
    let from_head = run_scene(ScatterFixture {
        head: Some(crate::sim::components::DriveCoord {
            x: 7 * 256 + 128,
            y: 7 * 256 + 128,
            z: 0,
        }),
        ..fixture()
    });
    assert_eq!(
        from_head.destination,
        Some((8, 6)),
        "scan follows Foot head; heading follows physical source"
    );
    let outside = run_scene(ScatterFixture {
        outside: true,
        ..fixture()
    });
    assert_eq!(outside.destination, None);
    assert!(
        outside.rng_changed,
        "selection failure keeps the preceding native draw"
    );

    // Seed 1 yields RandomRanged(0,4)==1. With the attacker due west,
    // native base direction is E (2), so start=NE (1) and the first open
    // cell is (6,4). The Move/NavCom write is already visible when fear is
    // subsequently latched to 300 for Fraidycat (native518C7B).
    let guard = run(MissionType::Guard, true, 125, true, Some(true), true, false);
    assert_eq!(guard.health, 115);
    assert_eq!(guard.destination, Some((6, 4)));
    assert_eq!(
        guard.queued_mission,
        MissionId::from_known(MissionType::Move)
    );
    assert_eq!(guard.fear, 300);
    assert!(guard.rng_changed);

    // A human's man hit without a source takes neither Scatter; the
    // computer's tail Scatter is the production receiver test's.
    let null_attacker = run(
        MissionType::Guard,
        false,
        125,
        true,
        Some(true),
        false,
        false,
    );
    assert_eq!(null_attacker.destination, None);
    assert!(!null_attacker.rng_changed);

    let fatal = run(MissionType::Guard, true, 10, true, None, false, false);
    assert_eq!(fatal.health, 0);
    assert_eq!(fatal.destination, None);

    let attack_mission = run(MissionType::Attack, true, 125, true, None, false, false);
    assert_eq!(attack_mission.destination, None);
    assert!(!attack_mission.rng_changed);

    // Unforced damage must not make an ordinary combat infantryman flee,
    // even if an AI owns it or its human owner has a Team and NavCom.
    for control in [None, Some(true), Some(false)] {
        let soldier = run(MissionType::Guard, true, 125, false, control, true, true);
        assert_eq!(soldier.health, 115);
        assert_eq!(soldier.destination, None);
        assert!(!soldier.rng_changed, "{soldier:?}");
    }
    for control in [Some(true), Some(false)] {
        let moving_civilian = run(MissionType::Guard, true, 125, true, control, false, true);
        assert_eq!(moving_civilian.destination, None);
        assert!(
            !moving_civilian.rng_changed,
            "NavCom cannot substitute for Team"
        );
        let team_civilian = run(MissionType::Guard, true, 125, true, control, true, false);
        assert_eq!(team_civilian.destination, Some((6, 4)));
        assert!(team_civilian.rng_changed);
    }
}

fn bridge_layer_test_fixture() -> (
    EntityStore,
    OccupancyGrid,
    ResolvedTerrainGrid,
    RuleSet,
    WarheadType,
    StringInterner,
) {
    let ini = IniFile::from_str(
        "\
[VehicleTypes]\n0=MTNK\n\n\
[InfantryTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[Warheads]\n0=BridgeSplash\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\n\
[BridgeSplash]\nCellSpread=1\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules parse");
    let warhead =
        WarheadType::from_ini_section("BridgeSplash", ini.section("BridgeSplash").unwrap());

    let ground = GameEntity::test_default(1, "MTNK", "Soviet", 5, 5);
    let mut bridge = GameEntity::test_default(2, "MTNK", "Soviet", 5, 5);
    bridge.set_on_bridge(true);
    bridge.position.set_z_for_test(4);

    let mut entities = EntityStore::new();
    entities.insert(ground);
    entities.insert(bridge);

    let mut occupancy = OccupancyGrid::new();
    prepend_ground(&mut occupancy, 5, 5, 1);
    occupancy.add(
        5,
        5,
        2,
        MovementLayer::Bridge,
        None,
        CellListInsertion::PrependNonBuilding,
    );

    let mut cells = Vec::new();
    for ry in 0..10 {
        for rx in 0..10 {
            cells.push(test_terrain_cell(rx, ry));
        }
    }
    let idx = 5 * 10 + 5;
    cells[idx].bridge_facts = BridgeCellFacts {
        raw_flags: BRIDGE_FLAG_STRUCTURAL,
        ..BridgeCellFacts::default()
    };
    cells[idx].has_bridge_deck = true;
    cells[idx].bridge_walkable = true;
    cells[idx].bridge_deck_level = 4;
    let terrain = ResolvedTerrainGrid::from_cells(10, 10, cells);

    test_intern("Americans");
    let interner = test_interner();

    (entities, occupancy, terrain, rules, warhead, interner)
}

fn test_terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
    }
}

#[test]
fn gsi_04_10_wood_splash_collects_terrain_in_native_receiver_order() {
    let ini = IniFile::from_str(
        "[General]\nTreeStrength=100\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=UNIT\n\
         [AircraftTypes]\n0=AIR\n\
         [BuildingTypes]\n0=BLDG\n\
         [TerrainTypes]\n0=TREE01\n\
         [Warheads]\n0=WOODWH\n\
         [UNIT]\nStrength=100\nArmor=none\n\
         [AIR]\nStrength=100\nArmor=none\n\
         [BLDG]\nStrength=100\nArmor=wood\n\
         [TREE01]\nStrength=100\nArmor=wood\nImmune=no\n\
         [WOODWH]\nWood=yes\nCellSpread=2\nPercentAtMax=.5\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("Terrain receiver-order rules");
    let warhead = rules.warhead("WOODWH").expect("wood warhead").clone();

    let mut entities = EntityStore::new();
    let mut unit = GameEntity::test_default(20, "UNIT", "Americans", 5, 5);
    unit.position.set_sub_x_for_test(CELL_CENTER_LEPTON);
    unit.position.set_sub_y_for_test(CELL_CENTER_LEPTON);
    entities.insert(unit);

    let building = GameEntity::test_default_of_category(
        30,
        "BLDG",
        "Americans",
        5,
        5,
        EntityCategory::Structure,
    );
    entities.insert(building);

    let mut air = GameEntity::test_default(10, "AIR", "Americans", 5, 5);
    air.set_category_for_test(EntityCategory::Aircraft);
    air.lifecycle.set_in_limbo_for_test(false);
    air.lifecycle.set_cell_marked_for_test(true);
    air.position.set_sub_x_for_test(CELL_CENTER_LEPTON);
    air.position.set_sub_y_for_test(CELL_CENTER_LEPTON);
    let mut air_locomotor = crate::sim::movement::locomotor::LocomotorState::from_object_type(
        rules.object("AIR").expect("air type"),
        0,
    );
    air_locomotor.set_layer_for_test(MovementLayer::Air);
    air.locomotor = Some(air_locomotor);
    air.position.set_exact_z_leptons_for_test(Some(1));
    air = air
        .with_air_spatial_membership_for_test(Some(5 + 5 * 20), 1)
        .with_air_tracker_cell_for_test((5, 5));
    entities.insert(air);
    // The entities' names are interned in the thread's test interner;
    // damage resolves them through these ids.
    let mut interner = crate::sim::intern::test_interner();
    let warhead_ref = interner.intern("WOODWH");
    let tree_ref = interner.intern("TREE01");

    let mut occupancy = OccupancyGrid::new();
    prepend_ground(&mut occupancy, 5, 5, 20);
    append_building(&mut occupancy, 5, 5, 30);

    let mut terrain_objects = crate::sim::terrain_object::TerrainObjectStore::default();
    for (stable_id, rx) in [(100, 5), (101, 6), (102, 8)] {
        terrain_objects.insert_constructed(
            {
                let mut terrain = TerrainObjectState::for_test(stable_id, tree_ref, rx, 5);
                terrain.set_occupation_bits_for_test(4);
                terrain
            },
            false,
        );
        terrain_objects.add_to_cell(stable_id, (rx, 5));
    }

    let mut terrain = crate::map::resolved_terrain::test_grid(12, 12, test_terrain_cell);
    let result = apply_aoe_damage_with_terrain(
        &mut entities,
        5,
        5,
        100,
        &warhead,
        &rules,
        &mut interner,
        (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
        native_lists(&occupancy, &mut terrain, Some(1)),
        Some(TerrainCollectionView {
            store: &terrain_objects,
        }),
    );

    let receiver_ids = result
        .receivers
        .iter()
        .map(|receiver| match receiver {
            AreaDamageReceiver::Entity(event) => ("entity", event.target_id),
            AreaDamageReceiver::Terrain(event) => ("terrain", event.stable_id),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        &receiver_ids[..4],
        &[
            ("entity", 10),
            ("entity", 20),
            ("terrain", 100),
            ("entity", 30),
        ],
        "air precedes the center list; Terrain stays after later nonbuildings and before Buildings"
    );
    let adjacent = result
        .receivers
        .iter()
        .find_map(|receiver| match receiver {
            AreaDamageReceiver::Terrain(event) if event.stable_id == 101 => Some(*event),
            _ => None,
        })
        .expect("an adjacent tree-only cell is collected without a Techno occupancy bucket");
    assert_eq!(adjacent.distance_leptons, 256);
    assert_eq!(
        adjacent.damage, 100,
        "the receiver record retains raw damage until ordered commit"
    );
    assert_eq!(
        super::super::damage::kernel::apply_warhead_damage(
            adjacent.damage,
            warhead.cell_spread_f64,
            warhead.percent_at_max_f64,
            warhead
                .interaction(crate::rules::armor::ArmorId::WOOD)
                .multiplier(),
            adjacent.distance_leptons,
            false,
            rules.combat_damage.max_damage,
        ),
        75,
        "the captured per-object distance produces the exact shared-kernel falloff at commit"
    );
    assert_eq!(
        result
            .receivers
            .iter()
            .filter(|receiver| {
                matches!(
                    receiver,
                    AreaDamageReceiver::Terrain(event) if event.stable_id == 100
                )
            })
            .count(),
        1,
        "the former exact-center Terrain sidecar must not duplicate the unified receiver"
    );
    assert!(
        !result.receivers.iter().any(|receiver| {
            matches!(
                receiver,
                AreaDamageReceiver::Terrain(event) if event.stable_id == 102
            )
        }),
        "Terrain outside the signed CellSpread radius is not captured"
    );
}

#[test]
fn gsi_04_10_spawning_terrain_c4_precedes_uninit_and_guards_reentry() {
    let ini = IniFile::from_str(
        "[General]\nTreeStrength=10\n\
         [CombatDamage]\nC4Warhead=C4WH\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=VICTIM\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [TerrainTypes]\n0=TIBTREE\n\
         [Warheads]\n0=OUTERWH\n1=C4WH\n\
         [VICTIM]\nStrength=1000\nArmor=wood\n\
         [TIBTREE]\nStrength=10\nArmor=wood\nImmune=no\nSpawnsTiberium=yes\n\
         [OUTERWH]\nWood=yes\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\
         [C4WH]\nWood=yes\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("spawning Terrain rules");
    let mut sim = crate::sim::world::Simulation::new();
    sim.resolve_type_handles(&rules);
    let victim_id = sim
        .spawn_object("VICTIM", "VictimHouse", 5, 5, 0, &rules)
        .expect("nested C4 victim spawns");

    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(10, 10, |rx, ry| {
        test_terrain_cell(rx, ry)
    }));

    let terrain_id = 900;
    let terrain_ref = sim.interner.intern("TIBTREE");
    let terrain_state = {
        let mut terrain = TerrainObjectState::for_test(terrain_id, terrain_ref, 5, 5);
        terrain.set_health_for_test(10);
        terrain.set_max_health_for_test(10);
        terrain.set_occupation_bits_for_test(4);
        terrain
    };
    sim.terrain_objects
        .objects_mut_for_test()
        .insert(terrain_id, terrain_state.clone());
    sim.terrain_objects
        .cell_index_mut_for_test()
        .insert((5, 5), terrain_id);
    sim.terrain_objects
        .spawning_cells_mut_for_test()
        .insert((5, 5));
    crate::sim::terrain_object::mark_terrain_raw_occupation(
        &mut sim.substrate.raw_cell_occupation,
        (5, 5),
        terrain_state.occupation_bits(),
    );
    sim.substrate.raw_cell_occupation.mark_ground(5, 5, 0x80);
    sim.substrate.raw_cell_occupation.mark_deck(5, 5, 0xA5);
    crate::sim::terrain_object::mark_terrain_occupation(
        &terrain_state,
        sim.resolved_terrain.as_mut(),
    );
    let initial_path = crate::sim::pathfinding::PathGrid::from_resolved_terrain(
        sim.resolved_terrain
            .as_ref()
            .expect("pre-damage resolved terrain"),
    );
    sim.rebuild_zone_grid(&initial_path);

    let outer_ref = sim.interner.intern("OUTERWH");
    let later_parent_receiver = EntityDamageEvent::area(
        victim_id,
        10,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        outer_ref,
    );
    sim.commit_noncombat_aoe_receivers(
        &rules,
        None,
        &[
            AreaDamageReceiver::Terrain(TerrainDamageEvent {
                damage_writeback: None,
                stable_id: terrain_id,
                rx: 5,
                ry: 5,
                damage: 10,
                distance_leptons: 0,
                warhead_ref: outer_ref,
                near_center_ic_isolation_eligible: true,
            }),
            AreaDamageReceiver::Entity(later_parent_receiver),
        ],
        &mut crate::sim::world::FrameEffects::default(),
    );

    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .health
            .current(),
        890,
        "nested raw-100 null-source C4 completes before the later parent receiver"
    );
    assert!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .attack_target
            .target()
            .is_none(),
        "nested and parent null-source events do not fabricate an attacker"
    );
    let destroyed = &sim.terrain_objects.objects()[&terrain_id];
    assert_eq!(destroyed.health(), 0);
    assert_eq!(destroyed.lifecycle(), TerrainObjectLifecycle::Destroyed);
    assert!(!sim.terrain_objects.cell_index().contains_key(&(5, 5)));
    assert!(
        !sim.terrain_objects.spawning_cells().contains(&(5, 5)),
        "the outer Terrain finalizes once after the Wood-enabled C4 re-entry is guarded"
    );
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(5, 5)
            & crate::sim::terrain_object::terrain_raw_occupation_mask(
                terrain_state.occupation_bits()
            ),
        0,
        "Terrain-specific raw occupation clears even though the live Techno remains"
    );
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(5, 5) & 0x80,
        0x80
    );
    assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(5, 5), 0xA5);
    let resolved = sim
        .resolved_terrain
        .as_ref()
        .expect("same-frame terrain refresh");
    let cell = resolved.cell(5, 5).expect("Terrain source cell");
    assert_eq!(cell.terrain_object_occupation, None);
    assert!(!cell.terrain_object_blocks);
    assert!(crate::sim::pathfinding::PathGrid::from_resolved_terrain(resolved).is_walkable(5, 5));
    assert!(
        sim.zone_grid.is_some(),
        "same-frame Terrain finalization rebuilds zones"
    );
}

#[test]
fn gsi_04_10_scenario_no_damage_blocks_entity_terrain_and_nested_c4() {
    let ini = IniFile::from_str(
        "[General]\nTreeStrength=10\n\
         [CombatDamage]\nC4Warhead=C4WH\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=VICTIM\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [TerrainTypes]\n0=TIBTREE\n\
         [Warheads]\n0=OUTERWH\n1=C4WH\n\
         [VICTIM]\nStrength=1000\nArmor=wood\n\
         [TIBTREE]\nStrength=10\nArmor=wood\nImmune=no\nSpawnsTiberium=yes\n\
         [OUTERWH]\nWood=yes\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\
         [C4WH]\nWood=yes\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("inert Terrain rules");
    let mut sim = crate::sim::world::Simulation::new();
    sim.resolve_type_handles(&rules);
    let victim_id = sim
        .spawn_object("VICTIM", "VictimHouse", 5, 5, 0, &rules)
        .expect("inert receiver victim spawns");
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(10, 10, |rx, ry| {
        test_terrain_cell(rx, ry)
    }));

    let terrain_id = 901;
    let terrain_ref = sim.interner.intern("TIBTREE");
    sim.terrain_objects
        .objects_mut_for_test()
        .insert(terrain_id, {
            let mut terrain = TerrainObjectState::for_test(terrain_id, terrain_ref, 5, 5);
            terrain.set_health_for_test(10);
            terrain.set_max_health_for_test(10);
            terrain.set_occupation_bits_for_test(4);
            terrain
        });
    sim.terrain_objects
        .cell_index_mut_for_test()
        .insert((5, 5), terrain_id);
    sim.terrain_objects
        .spawning_cells_mut_for_test()
        .insert((5, 5));
    sim.session.set_no_damage_for_test(true);
    let outer_ref = sim.interner.intern("OUTERWH");
    let before_hash = sim.state_hash();
    let before_rng = sim.scenario_rng.state();

    sim.commit_noncombat_aoe_receivers(
        &rules,
        None,
        &[
            AreaDamageReceiver::Entity(EntityDamageEvent::direct_receiver(
                victim_id,
                100,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                outer_ref,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: false,
                    arg6: true,
                },
            )),
            AreaDamageReceiver::Terrain(TerrainDamageEvent {
                damage_writeback: None,
                stable_id: terrain_id,
                rx: 5,
                ry: 5,
                damage: 10,
                distance_leptons: 0,
                warhead_ref: outer_ref,
                near_center_ic_isolation_eligible: true,
            }),
        ],
        &mut crate::sim::world::FrameEffects::default(),
    );

    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .health
            .current(),
        1000
    );
    let terrain = &sim.terrain_objects.objects()[&terrain_id];
    assert_eq!(terrain.health(), 10);
    assert_eq!(terrain.lifecycle(), TerrainObjectLifecycle::Live);
    assert_eq!(sim.terrain_objects.cell_index()[&(5, 5)], terrain_id);
    assert_eq!(sim.scenario_rng.state(), before_rng);
    assert_eq!(sim.state_hash(), before_hash);
}
