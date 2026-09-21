//! Tests for `Simulation::tick_attack_pursuit` — the pre-combat stage
//! that walks units toward out-of-range attack targets and halts them
//! when in range.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::aircraft::AircraftMission;
use crate::sim::combat::AttackTarget;
use crate::sim::components::Health;
use crate::sim::docking::aircraft_dock::AircraftAmmo;
use crate::sim::game_entity::GameEntity;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

/// Minimal RuleSet for pursuit tests: armed Grizzly + Rhino, AP warhead with
/// non-zero Verses against heavy. Range=6 cells.
fn pursuit_rules() -> RuleSet {
    let ini_str: &str = "\
[VehicleTypes]\n0=MTNK\n1=HTNK\n\n\
[InfantryTypes]\n0=ENGI\n\n\
[BuildingTypes]\n0=GAPILL\n\n\
[AircraftTypes]\n0=ORCA\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
[HTNK]\nStrength=400\nArmor=heavy\nSpeed=5\nPrimary=105mm\n\n\
[ENGI]\nStrength=75\nArmor=none\nSpeed=4\n\n\
[GAPILL]\nStrength=400\nArmor=heavy\nPrimary=105mm\n\n\
[ORCA]\nStrength=150\nArmor=light\nSpeed=14\nPrimary=105mm\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n";
    let ini: IniFile = IniFile::from_str(ini_str);
    RuleSet::from_ini(&ini).expect("pursuit_rules should parse")
}

/// Construct a Simulation with a flat 64x64 PathGrid and the given entities
/// pre-inserted. Returns the sim plus the path grid (kept alive separately
/// because tick_attack_pursuit borrows it).
///
/// Replaces the sim's interner with the thread-local test interner so the
/// type_ref / owner IDs that `GameEntity::test_default` baked in via
/// `test_intern()` resolve correctly.
fn make_sim(entities: Vec<GameEntity>) -> (Simulation, PathGrid) {
    let mut sim = Simulation::new();
    for e in entities {
        sim.substrate.entities.insert(e);
    }
    sim.interner = crate::sim::intern::test_interner();
    let grid = PathGrid::test_all_passable(64, 64);
    (sim, grid)
}

fn make_unit(id: u64, type_ref: &str, owner: &str, rx: u16, ry: u16, hp: i32) -> GameEntity {
    let mut e = GameEntity::test_default(id, type_ref, owner, rx, ry);
    e.health = Health { current: hp };
    e
}

#[test]
fn cell_target_out_of_range_issues_movement() {
    // Grizzly at (5,5), force-fire Cell(15,15). Range=6, distance=10 → out of range.
    let mut grizzly = make_unit(1, "MTNK", "Americans", 5, 5, 300);
    grizzly.attack_target = Some(AttackTarget::for_cell(15, 15));
    let (mut sim, grid) = make_sim(vec![grizzly]);
    let rules = pursuit_rules();

    sim.tick_attack_pursuit(&rules, Some(&grid));

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(
        entity.attack_target.is_some(),
        "attack_target preserved during pursuit"
    );
    assert!(
        entity.movement_target.is_some(),
        "out-of-range cell target should issue movement"
    );
}

#[test]
fn cell_target_in_range_clears_movement() {
    // Grizzly at (8,5), force-fire Cell(10,5). Distance=2 → in range.
    // Pre-set a movement_target as if pursuit had issued one earlier.
    let mut grizzly = make_unit(1, "MTNK", "Americans", 8, 5, 300);
    grizzly.attack_target = Some(AttackTarget::for_cell(10, 5));
    grizzly.movement_target = Some(crate::sim::components::MovementTarget::default());
    let (mut sim, grid) = make_sim(vec![grizzly]);
    let rules = pursuit_rules();

    sim.tick_attack_pursuit(&rules, Some(&grid));

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(
        entity.attack_target.is_some(),
        "attack_target preserved on range entry"
    );
    assert!(
        entity.movement_target.is_none(),
        "in-range pursuit should halt movement"
    );
}

#[test]
fn entity_target_out_of_range_pursues() {
    // Grizzly at (0,0) attacking Rhino at (10,0). Out of range.
    let mut grizzly = make_unit(1, "MTNK", "Americans", 0, 0, 300);
    grizzly.attack_target = Some(AttackTarget::new(2));
    let rhino = make_unit(2, "HTNK", "Soviet", 10, 0, 400);
    let (mut sim, grid) = make_sim(vec![grizzly, rhino]);
    let rules = pursuit_rules();

    sim.tick_attack_pursuit(&rules, Some(&grid));

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.attack_target.is_some());
    assert!(
        entity.movement_target.is_some(),
        "out-of-range entity target should issue movement"
    );
}

#[test]
fn entity_target_dying_pursuit_skips() {
    // Target marked dying — resolve_target_coords still resolves, but combat
    // tick will clean up. Pursuit should not crash here.
    let mut grizzly = make_unit(1, "MTNK", "Americans", 0, 0, 300);
    grizzly.attack_target = Some(AttackTarget::new(2));
    let mut rhino = make_unit(2, "HTNK", "Soviet", 10, 0, 0);
    rhino.dying = true;
    rhino.health.current = 0;
    let (mut sim, grid) = make_sim(vec![grizzly, rhino]);
    let rules = pursuit_rules();

    sim.tick_attack_pursuit(&rules, Some(&grid));
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .attack_target
            .is_some()
    );
}

#[test]
fn aircraft_attack_target_skipped_by_pursuit() {
    // Aircraft has its own attack-mission state machine; pursuit must not
    // touch its movement.
    let mut orca = make_unit(1, "ORCA", "Americans", 0, 0, 150);
    orca.attack_target = Some(AttackTarget::new(2));
    orca.aircraft_mission = Some(AircraftMission::Attack { sub_state: 3 });
    orca.aircraft_ammo = Some(AircraftAmmo::new(2));
    let rhino = make_unit(2, "HTNK", "Soviet", 30, 0, 400);
    let (mut sim, grid) = make_sim(vec![orca, rhino]);
    let rules = pursuit_rules();

    sim.tick_attack_pursuit(&rules, Some(&grid));

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(
        entity.movement_target.is_none(),
        "aircraft pursuit must not be touched by ground pursuit stage"
    );
}

#[test]
fn structure_attack_target_skipped_by_pursuit() {
    // Garrisoned building (or any structure) has attack_target but cannot move.
    let mut pillbox = make_unit(1, "GAPILL", "Americans", 5, 5, 400);
    pillbox.category = crate::map::entities::EntityCategory::Structure;
    pillbox.attack_target = Some(AttackTarget::new(2));
    let rhino = make_unit(2, "HTNK", "Soviet", 30, 5, 400);
    let (mut sim, grid) = make_sim(vec![pillbox, rhino]);
    let rules = pursuit_rules();

    sim.tick_attack_pursuit(&rules, Some(&grid));

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(
        entity.movement_target.is_none(),
        "structures must not pursue"
    );
}

#[test]
fn deployed_infantry_skipped_by_pursuit() {
    // Deploy-fire infantry (e.g., GI in deployed state) cannot move.
    let mut gi = make_unit(1, "ENGI", "Americans", 5, 5, 75);
    gi.category = crate::map::entities::EntityCategory::Infantry;
    gi.deploy_state = Some(crate::sim::deploy::DeployPhase::Deployed);
    gi.attack_target = Some(AttackTarget::new(2));
    let rhino = make_unit(2, "HTNK", "Soviet", 30, 5, 400);
    let (mut sim, grid) = make_sim(vec![gi, rhino]);
    let rules = pursuit_rules();

    sim.tick_attack_pursuit(&rules, Some(&grid));

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(
        entity.movement_target.is_none(),
        "deployed infantry must not pursue"
    );
}

#[test]
fn pursuit_uses_same_range_as_combat_no_oscillation() {
    // Place attacker exactly at the boundary. The combat tick range check
    // and pursuit range check use the same `is_within_range_leptons`, so
    // both must agree at the boundary. Verify: at exactly Range cells,
    // pursuit treats it as in-range (clears movement if any).
    //
    // 105mm Range=6. Place Grizzly at (0,0), target Cell(6,0). Distance = 6 cells exactly.
    let mut grizzly = make_unit(1, "MTNK", "Americans", 0, 0, 300);
    grizzly.attack_target = Some(AttackTarget::for_cell(6, 0));
    grizzly.movement_target = Some(crate::sim::components::MovementTarget::default());
    let (mut sim, grid) = make_sim(vec![grizzly]);
    let rules = pursuit_rules();

    sim.tick_attack_pursuit(&rules, Some(&grid));

    let entity = sim.substrate.entities.get(1).unwrap();
    // is_within_range_leptons is inclusive at the boundary. Pursuit should
    // halt (clear movement). If pursuit and combat used different math,
    // this would fail.
    assert!(
        entity.movement_target.is_none(),
        "at exactly weapon range, pursuit must halt (matches combat tick range check)"
    );
}

/// **Sticky never chases.** Guard(5) and Sticky(6) share one mission handler,
/// and the single place the engine tells them apart is here: when the object
/// cannot already fire at its target, a Sticky object drops both the target and
/// the destination and produces no pursuit cell — ahead of the fallthrough that
/// lets a Guard-family object pursue. That is the whole of `[Sticky]`'s "just
/// like guard mode, but cannot move". Stock skirmish maps park neutral civilian
/// traffic on this mission (46 authored placements across the stock MP bundle,
/// 17 on one map), so without it a shot-at civilian truck drives at the shooter.
#[test]
fn sticky_drops_the_target_instead_of_chasing_it() {
    let mut civilian = make_unit(1, "MTNK", "Americans", 0, 0, 300);
    civilian.attack_target = Some(AttackTarget::new(2));
    civilian
        .mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: crate::sim::mission::MissionId::from_known(
                crate::sim::mission::MissionType::Sticky,
            ),
            suspended: crate::sim::mission::MissionId::NONE,
            queued: crate::sim::mission::MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
        });
    // 105mm Range=6; the Rhino sits at 10 cells, so the can-fire-at query fails.
    let rhino = make_unit(2, "HTNK", "Soviet", 10, 0, 400);
    let (mut sim, grid) = make_sim(vec![civilian, rhino]);
    let rules = pursuit_rules();

    sim.tick_attack_pursuit(&rules, Some(&grid));

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(
        entity.attack_target.is_none(),
        "Sticky drops the target it cannot shoot"
    );
    assert!(
        entity.movement_target.is_none(),
        "Sticky produces no pursuit cell"
    );
    assert!(entity.navigation.nav_com.is_none());
}

/// The same object on Guard — the mission Sticky shares its handler with —
/// still pursues. This is the tripwire proving the clause keys on the mission
/// id and not on something both missions share.
#[test]
fn guard_still_chases_where_sticky_would_not() {
    let mut guard = make_unit(1, "MTNK", "Americans", 0, 0, 300);
    guard.attack_target = Some(AttackTarget::new(2));
    guard
        .mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: crate::sim::mission::MissionId::from_known(
                crate::sim::mission::MissionType::Guard,
            ),
            suspended: crate::sim::mission::MissionId::NONE,
            queued: crate::sim::mission::MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
        });
    let rhino = make_unit(2, "HTNK", "Soviet", 10, 0, 400);
    let (mut sim, grid) = make_sim(vec![guard, rhino]);
    let rules = pursuit_rules();

    sim.tick_attack_pursuit(&rules, Some(&grid));

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.attack_target.is_some());
    assert!(
        entity.movement_target.is_some(),
        "Guard is not short-circuited"
    );
}

// ---------------------------------------------------------------------------
// The pursuit predicate is the fire gate's predicate, walk included.
//
// `FootClass::Mission_Attack @ 0x004D4DC0` dispatches to the approach search
// through vtable slot `+0x53C` (`CALL [EAX+0x53c]` at `0x004D4E6A`; the body is
// `FootClass::Greatest_Threat_Scan @ 0x004D5690`, and InfantryClass's override
// `0x00522340` chains into it at `0x0052236E`), and that body decides with
// `TechnoClass::InRange @ 0x006F7220`
// — called at `0x004D622C` and `0x004D6550` with a candidate coordinate as arg1
// and TarCom as arg2. `InRange` ends in the wall/cliff walk
// (`CALL 0x004CC310` at `0x006F7642`). So in gamemd the approach test and the
// fire test are literally the same call, and VERA's two stages must not use
// different predicates: if pursuit measured the plain radius while the fire
// gate ran the walk, a unit ordered to shoot across a wall would halt here and
// then be refused the shot, freezing under a live order.
// ---------------------------------------------------------------------------

/// Rules for the wall cases: a `SubjectToWalls=yes` projectile whose warhead
/// carries no `Wall=`, so `0x004CC342` cannot re-admit the blocked shot.
///
/// `[OverlayTypes]` is read by DECLARATION index (`RulesClass::Process`
/// `XOR EBX,EBX` at `0x00668CF3`, `PUSH EBX` at `0x00668D0A`), so `GAWALL` has
/// to be the third entry to land on the id `IsWallConnectableInDirection`
/// 0x00480510 accepts.
fn wall_pursuit_rules() -> RuleSet {
    let ini_str: &str = "\
[OverlayTypes]\n1=GASAND\n2=CYCL\n3=GAWALL\n\n\
[GASAND]\nWall=yes\n\n[CYCL]\n\n[GAWALL]\nWall=yes\n\n\
[VehicleTypes]\n0=MTNK\n1=HTNK\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
[HTNK]\nStrength=400\nArmor=heavy\nSpeed=5\nPrimary=105mm\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nProjectile=CANNON\nWarhead=AP\n\n\
[CANNON]\nSubjectToWalls=yes\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n";
    let ini: IniFile = IniFile::from_str(ini_str);
    RuleSet::from_ini(&ini).expect("wall_pursuit_rules should parse")
}

fn wall_pursuit_registry() -> crate::map::overlay_types::OverlayTypeRegistry {
    let ini_str: &str = "\
[OverlayTypes]\n1=GASAND\n2=CYCL\n3=GAWALL\n\n\
[GASAND]\nWall=yes\n\n[CYCL]\n\n[GAWALL]\nWall=yes\n";
    crate::map::overlay_types::OverlayTypeRegistry::from_ini(&IniFile::from_str(ini_str), None)
}

const WALL_TEST_GRID: u16 = 64;
/// `GAWALL`'s declaration index in the fixture's `[OverlayTypes]`.
const WALL_TEST_GAWALL_ID: u8 = 2;

fn wall_test_cell(rx: u16, ry: u16) -> crate::map::resolved_terrain::ResolvedTerrainCell {
    crate::map::resolved_terrain::ResolvedTerrainCell {
        rx,
        ry,
        source_tile_index: 0,
        source_sub_tile: 0,
        final_tile_index: 0,
        final_sub_tile: 0,
        is_wood_bridge_repair_tile: false,
        level: 0,
        filled_clear: true,
        tileset_index: Some(0),
        land_type: 0,
        yr_cell_land_type: 0,
        slope_type: 0,
        template_height: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class: Default::default(),
        speed_costs: Default::default(),
        is_water: false,
        is_cliff_like: false,
        is_rough: false,
        is_road: false,
        height_in_pixels: 0,
        variant: 0,
        has_ramp: false,
        canonical_ramp: None,
        ground_walk_blocked: false,
        terrain_object_blocks: false,
        terrain_object_occupation: None,
        overlay_blocks: false,
        overlay_zone_type: None,
        outside_playfield: false,
        zone_type: 0,
        base_ground_walk_blocked: false,
        base_build_blocked: false,
        base_land_type: 0,
        base_yr_cell_land_type: 0,
        base_terrain_class: Default::default(),
        base_speed_costs: Default::default(),
        build_blocked: false,
        has_bridge_deck: false,
        bridge_walkable: false,
        bridge_transition: false,
        bridge_deck_level: 0,
        bridge_layer: None,
        bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
        tube_index: None,
        radar_left: [0; 3],
        radar_right: [0; 3],
        accepts_smudge: true,
        allows_tiberium: false,
        has_damaged_data: false,
        bridgehead_anchor_class_at_load: None,
    }
}

/// Flat terrain plus an overlay plane carrying the listed walls, installed on
/// the sim so `tick_attack_pursuit_with_overlay_registry` can run the 3-D gate.
fn install_wall_map(sim: &mut Simulation, walls: &[(u16, u16)]) {
    let cells: Vec<crate::map::resolved_terrain::ResolvedTerrainCell> = (0..WALL_TEST_GRID)
        .flat_map(|ry| (0..WALL_TEST_GRID).map(move |rx| wall_test_cell(rx, ry)))
        .collect();
    sim.resolved_terrain = Some(
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
            WALL_TEST_GRID,
            WALL_TEST_GRID,
            cells,
        ),
    );
    let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(WALL_TEST_GRID, WALL_TEST_GRID);
    for &(rx, ry) in walls {
        overlays.cell_mut(rx, ry).overlay_id = Some(WALL_TEST_GAWALL_ID);
    }
    sim.overlay_grid = Some(overlays);
}

/// **The deadlock tripwire.** Grizzly at (2,5), Rhino at (6,5): four cells, so
/// the plain radius says "in range" against `Range=6`. A `GAWALL` sits at (4,5),
/// and `[CANNON]` is `SubjectToWalls=yes` while `[AP]` has no `Wall=`, so the
/// fire gate refuses the shot at `0x006F7642`.
///
/// If pursuit judged range with the 2-D twin it would produce no pursuit cell,
/// the fire gate would refuse, and the tank would stand still under a live
/// attack order forever. Native never gets there: its approach routine measures
/// with the same `InRange` that runs the walk, so it keeps repositioning.
#[test]
fn a_wall_on_the_line_keeps_pursuit_closing_instead_of_freezing() {
    let mut grizzly = make_unit(1, "MTNK", "Americans", 2, 5, 300);
    grizzly.attack_target = Some(AttackTarget::new(2));
    let rhino = make_unit(2, "HTNK", "Soviet", 6, 5, 400);
    let (mut sim, grid) = make_sim(vec![grizzly, rhino]);
    install_wall_map(&mut sim, &[(4, 5)]);
    let rules = wall_pursuit_rules();
    let registry = wall_pursuit_registry();

    sim.tick_attack_pursuit_with_overlay_registry(
        &rules,
        Some(&grid),
        Some(&registry),
        &std::collections::BTreeSet::new(),
    );

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(
        entity.attack_target.is_some(),
        "the order survives — pursuit never drops a target it is still closing on"
    );
    assert!(
        entity.movement_target.is_some(),
        "a shot the fire gate refuses must keep pursuit moving, not freeze the unit"
    );
}

/// The control: the same fixture with the wall removed still halts, so the
/// tripwire above is pinning the walk and not merely "pursuit always moves".
#[test]
fn without_the_wall_the_same_shot_halts_pursuit() {
    let mut grizzly = make_unit(1, "MTNK", "Americans", 2, 5, 300);
    grizzly.attack_target = Some(AttackTarget::new(2));
    grizzly.movement_target = Some(crate::sim::components::MovementTarget::default());
    let rhino = make_unit(2, "HTNK", "Soviet", 6, 5, 400);
    let (mut sim, grid) = make_sim(vec![grizzly, rhino]);
    install_wall_map(&mut sim, &[]);
    let rules = wall_pursuit_rules();
    let registry = wall_pursuit_registry();

    sim.tick_attack_pursuit_with_overlay_registry(
        &rules,
        Some(&grid),
        Some(&registry),
        &std::collections::BTreeSet::new(),
    );

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(
        entity.movement_target.is_none(),
        "with a clear line at four cells the shot is legal, so pursuit halts to fire"
    );
}

/// A `MinimumRange` refusal must NOT produce a pursuit cell.
///
/// Native's approach search 0x004D5690 scans candidate coordinates and takes
/// one where `InRange` holds, so a V3 that has been closed on backs off. VERA's
/// pursuit has exactly one candidate — the target's own cell — which for a
/// too-close refusal is the worst cell in the set. Feeding the fire gate's full
/// verdict into pursuit without this arm would send the V3 driving AT the tank
/// it cannot shell, which is a worse symptom than the hold it replaces.
///
/// `MinimumRange=` is ordinary stock data: `V3Launcher` 5, `DredLauncher` and
/// `CruiseLauncher` 8, `MagneticBeam` 3, `HowitzerGun` 2, the
/// `MissileLauncher`/`HoverMissile` family 1.
#[test]
fn inside_minimum_range_pursuit_holds_instead_of_closing() {
    let ini_str: &str = "\
[VehicleTypes]\n0=MTNK\n1=HTNK\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=LOBBER\n\n\
[HTNK]\nStrength=400\nArmor=heavy\nSpeed=5\nPrimary=LOBBER\n\n\
[LOBBER]\nDamage=200\nROF=150\nRange=20\nMinimumRange=5\nWarhead=AP\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n";
    let rules = RuleSet::from_ini(&IniFile::from_str(ini_str)).expect("min-range rules parse");

    // Four cells apart: inside MinimumRange=5, well inside Range=20.
    let mut lobber = make_unit(1, "MTNK", "Americans", 2, 5, 300);
    lobber.attack_target = Some(AttackTarget::new(2));
    let rhino = make_unit(2, "HTNK", "Soviet", 6, 5, 400);
    let (mut sim, grid) = make_sim(vec![lobber, rhino]);
    install_wall_map(&mut sim, &[]);

    sim.tick_attack_pursuit_with_overlay_registry(
        &rules,
        Some(&grid),
        None,
        &std::collections::BTreeSet::new(),
    );

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(
        entity.attack_target.is_some(),
        "the order survives a too-close refusal"
    );
    assert!(
        entity.movement_target.is_none(),
        "inside MinimumRange pursuit must hold, never drive at the target"
    );
}

fn walk_pursuit_scene() -> (Simulation, RuleSet, u64, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=1000\nArmor=none\nSpeed=4\nSight=8\nPrimary=Rifle\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [Rifle]\nDamage=0\nROF=100\nRange=3\nProjectile=Bullet\nWarhead=SA\n\
         [Bullet]\nAG=yes\nAA=no\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=0%\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(0x75bd25);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    for (index, (name, human)) in [("Local", true), ("Enemy", false)].into_iter().enumerate() {
        let owner = sim.interner.intern(name);
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, index as u8, None, human, 0, 10),
        );
        sim.session.house_order.push(owner);
    }
    sim.session.current_house = sim.interner.get("Local");
    let costs = rules
        .terrain_rules
        .semantics_by_name("Clear")
        .unwrap()
        .speed_costs;
    let cells = (0..64)
        .flat_map(|y| {
            (0..64).map(move |x| {
                let mut cell = wall_test_cell(x, y);
                cell.speed_costs = costs;
                cell.base_speed_costs = costs;
                cell
            })
        })
        .collect();
    sim.install_resolved_terrain_for_new_map(
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(64, 64, cells),
    );
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let heights = std::collections::BTreeMap::new();
    let actor = sim
        .spawn_object("E1", "Local", 10, 10, 0, &rules, &heights)
        .unwrap();
    // Production visibility is recomputed by the frame host, not by spawn.
    // Establish the actor's sight before introducing an enemy or an order.
    walk_frame(&mut sim, &rules);
    let victim = sim
        .spawn_object("E1", "Enemy", 16, 10, 0, &rules, &heights)
        .unwrap();
    (sim, rules, actor, victim)
}

fn walk_frame(sim: &mut Simulation, rules: &RuleSet) {
    let grid = sim.path_grid_snapshot();
    sim.advance_tick(
        &[],
        Some(rules),
        &std::collections::BTreeMap::new(),
        grid.as_deref(),
        None,
        67,
    );
}

fn walk_command(sim: &mut Simulation, rules: &RuleSet, command: crate::sim::command::Command) {
    let grid = sim.path_grid_snapshot();
    assert!(sim.apply_command_with_overlays(
        "Local",
        &command,
        Some(rules),
        grid.as_deref(),
        &std::collections::BTreeMap::new(),
        None,
    ));
}

fn wait_for_walk_head(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
) -> crate::sim::components::DriveCoord {
    for _ in 0..50 {
        walk_frame(sim, rules);
        let loco = sim
            .substrate
            .entities
            .get(id)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap();
        if let Some(head) = loco.step_head() {
            assert_eq!(loco.walk_is_moving(), Some(true));
            assert_eq!(loco.walk_animation_moving(), Some(true));
            return head;
        }
    }
    panic!("real command/frame path must produce a paid Walk head");
}

#[test]
fn walk_destination_search_observes_route_opened_before_process() {
    let (mut sim, rules, actor, _) = walk_pursuit_scene();
    let open_grid = sim.path_grid_snapshot().unwrap();
    // Controlled navigation input: no route crosses this full-height barrier
    // when the order is accepted. The canonical map used by the subsequent
    // object turn is open. This distinguishes deferred search from storing an
    // order-time route (or refusing the accepted destination on A* failure).
    let mut closed_grid = (*open_grid).clone();
    for y in 0..closed_grid.height() {
        closed_grid.set_blocked(12, y, true);
        assert!(!closed_grid.is_any_layer_walkable(12, y));
    }
    assert!(sim.apply_command_with_overlays(
        "Local",
        &crate::sim::command::Command::Move {
            entity_id: actor,
            target_rx: 14,
            target_ry: 10,
            queue: false,
            group_id: None,
        },
        Some(&rules),
        Some(&closed_grid),
        &std::collections::BTreeMap::new(),
        None,
    ));
    let e = sim.substrate.entities.get(actor).unwrap();
    assert!(e.movement_target.as_ref().unwrap().path.is_empty());
    assert_eq!(
        e.navigation.nav_com,
        Some(crate::sim::components::NavTargetRef::Cell { rx: 14, ry: 10 })
    );
    assert!(e.locomotor.as_ref().unwrap().step_head().is_none());
    walk_frame(&mut sim, &rules);
    let e = sim.substrate.entities.get(actor).unwrap();
    assert!(e.locomotor.as_ref().unwrap().step_head().is_some());
    assert!(
        e.movement_target.as_ref().unwrap().path.contains(&(12, 10)),
        "the first Process searches the now-open route"
    );

    // Scatter's prepublished setter must also leave an execution request,
    // even though its caller ignores the helper's return value.
    let (mut sim, _rules, actor, _) = walk_pursuit_scene();
    assert!(crate::sim::movement::prepare_walk_cell_destination(
        &mut sim.substrate.entities,
        &closed_grid,
        actor,
        (14, 10),
        crate::util::fixed_math::SimFixed::from_num(4),
        None,
        sim.resolved_terrain.as_ref(),
        sim.zone_grid.as_ref(),
        sim.playfield_bounds,
        &mut sim.substrate.cell_occupation,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    let e = sim.substrate.entities.get(actor).unwrap();
    assert!(e.movement_target.as_ref().unwrap().path.is_empty());
    assert!(e.navigation.nav_com.is_some());
    assert!(e.locomotor.as_ref().unwrap().walk_destination().is_some());
}

#[test]
fn walk_cell_order_defers_queue_publication_and_first_head_motion() {
    use crate::sim::components::{FootPathQueue, NavTargetRef};
    let (mut sim, rules, actor, _) = walk_pursuit_scene();
    let queue = FootPathQueue {
        directions: vec![2, 3, 4, 5],
        cursor: 0,
        reference_cell: Some((9, 8)),
    };
    let entity = sim.substrate.entities.get_mut(actor).unwrap();
    entity.navigation.path_replay = queue.clone();
    entity.navigation.nav_queue = vec![NavTargetRef::Cell { rx: 25, ry: 10 }];
    let before = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    walk_command(
        &mut sim,
        &rules,
        crate::sim::command::Command::Move {
            entity_id: actor,
            target_rx: 14,
            target_ry: 10,
            queue: false,
            group_id: None,
        },
    );
    let entity = sim.substrate.entities.get(actor).unwrap();
    let mut invalidated = queue;
    invalidated.clear_live_head();
    assert_eq!(
        entity.navigation.path_replay, invalidated,
        "accepted setter writes one native path head"
    );
    assert_eq!(
        entity.navigation.nav_queue,
        vec![NavTargetRef::Cell { rx: 25, ry: 10 }]
    );
    assert_eq!(entity.locomotor.as_ref().unwrap().step_head(), None);
    walk_frame(&mut sim, &rules);
    let entity = sim.substrate.entities.get(actor).unwrap();
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(&entity.position),
        before,
        "the original fresh-head Process returns before numerical motion"
    );
    assert!(entity.locomotor.as_ref().unwrap().step_head().is_some());
    assert_eq!(
        entity.locomotor.as_ref().unwrap().walk_animation_moving(),
        Some(true)
    );
    assert!(
        !entity
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty()
    );
    assert_eq!(entity.navigation.path_replay.reference_cell, Some((10, 10)));
    assert_eq!(
        entity.foot_speed.applied_fraction,
        crate::util::fixed_math::SIM_ONE
    );
    walk_frame(&mut sim, &rules);
    assert_ne!(
        crate::sim::movement::ground_pose::position_world_coord(
            &sim.substrate.entities.get(actor).unwrap().position
        ),
        before,
        "the subsequent paid-head Process advances"
    );
}

#[test]
fn walk_pursuit_range_entry_finishes_paid_head_then_accepts_new_move() {
    use crate::sim::command::Command;
    let (mut sim, rules, actor, victim) = walk_pursuit_scene();
    let owner = sim.substrate.entities.get(actor).unwrap().owner();
    assert!(
        sim.fog.is_cell_visible(owner, 16, 10),
        "the ordered target starts outside Range3 but inside Sight8"
    );
    walk_command(
        &mut sim,
        &rules,
        Command::Attack {
            attacker_id: actor,
            target_id: victim,
        },
    );
    let head = wait_for_walk_head(&mut sim, &rules, actor);
    assert_eq!(
        sim.substrate
            .entities
            .get(actor)
            .unwrap()
            .attack_target
            .as_ref()
            .map(|t| t.target),
        Some(crate::sim::combat::TargetKind::Entity(victim)),
        "the real first-step path retains the visible attack target"
    );
    // Controlled opponent movement input, using the actual membership writers.
    // The mover's command/Process/head and the subsequent range call are real.
    sim.remove_entity_occupancy(victim);
    {
        let e = sim.substrate.entities.get_mut(victim).unwrap();
        e.position.rx = (head.x / 256 + 2) as u16;
        e.position.ry = (head.y / 256) as u16;
        e.position.exact_z_leptons = Some(0);
    }
    sim.add_entity_occupancy(victim);
    let grid = sim.path_grid_snapshot();
    sim.tick_attack_pursuit(&rules, grid.as_deref());
    let e = sim.substrate.entities.get(actor).unwrap();
    assert_eq!(e.locomotor.as_ref().unwrap().step_head(), Some(head));
    assert!(
        e.movement_target.is_some(),
        "range entry must not strand the paid head"
    );
    assert!(e.navigation.nav_com.is_some());
    for _ in 0..80 {
        walk_frame(&mut sim, &rules);
        if sim
            .substrate
            .entities
            .get(actor)
            .unwrap()
            .movement_target
            .is_none()
        {
            break;
        }
    }
    let e = sim.substrate.entities.get(actor).unwrap();
    assert!(
        e.movement_target.is_none(),
        "completed-head PerCell range stops pursuit"
    );
    assert_eq!(e.locomotor.as_ref().unwrap().step_head(), None);
    assert_eq!(e.locomotor.as_ref().unwrap().walk_destination(), None);
    assert_eq!(e.locomotor.as_ref().unwrap().walk_is_moving(), Some(false));
    assert_eq!(
        e.locomotor.as_ref().unwrap().walk_animation_moving(),
        Some(false)
    );
    assert!(e.navigation.nav_com.is_none());
    assert_eq!(
        (e.position.rx, e.position.ry),
        ((head.x / 256) as u16, (head.y / 256) as u16)
    );
    let from = (e.position.rx, e.position.ry);
    let raw_bits: u32 = sim
        .substrate
        .raw_cell_occupation
        .entries()
        .map(|(_, _, ground, deck, _, _)| {
            u32::from(ground & 0x1c).count_ones() + u32::from(deck & 0x1c).count_ones()
        })
        .sum();
    assert_eq!(
        raw_bits, 2,
        "only the two current Infantry positions remain marked"
    );
    walk_command(
        &mut sim,
        &rules,
        Command::Move {
            entity_id: actor,
            target_rx: from.0,
            target_ry: from.1 + 4,
            queue: false,
            group_id: None,
        },
    );
    let next_head = wait_for_walk_head(&mut sim, &rules, actor);
    assert_ne!(
        next_head, head,
        "a later Move must not resurrect the retired head"
    );
}

#[test]
fn ordered_walk_attack_nulls_destination_but_preserves_paid_head_and_queues() {
    use crate::sim::command::Command;
    use crate::sim::components::{FootPathQueue, NavTargetRef};
    for force_cell in [false, true] {
        let (mut sim, rules, actor, victim) = walk_pursuit_scene();
        walk_command(
            &mut sim,
            &rules,
            Command::Move {
                entity_id: actor,
                target_rx: 20,
                target_ry: 10,
                queue: false,
                group_id: None,
            },
        );
        let head = wait_for_walk_head(&mut sim, &rules, actor);
        // Distinguish the two Foot queues using supplied existing backing state.
        let queue = FootPathQueue {
            directions: vec![6, 2, 3, 4, 5],
            cursor: 1,
            reference_cell: Some((9, 10)),
        };
        let e = sim.substrate.entities.get_mut(actor).unwrap();
        e.navigation.path_replay = queue.clone();
        e.navigation
            .nav_queue
            .push(NavTargetRef::Cell { rx: 25, ry: 10 });
        let queued = e.navigation.nav_queue.clone();
        walk_command(
            &mut sim,
            &rules,
            if force_cell {
                Command::ForceAttackCell {
                    attacker_id: actor,
                    target_rx: 12,
                    target_ry: 10,
                }
            } else {
                Command::Attack {
                    attacker_id: actor,
                    target_id: victim,
                }
            },
        );
        let e = sim.substrate.entities.get(actor).unwrap();
        assert_eq!(e.locomotor.as_ref().unwrap().step_head(), Some(head));
        assert_eq!(e.locomotor.as_ref().unwrap().walk_destination(), None);
        assert_eq!(e.locomotor.as_ref().unwrap().walk_is_moving(), Some(true));
        assert_eq!(
            e.locomotor.as_ref().unwrap().walk_animation_moving(),
            Some(true)
        );
        assert!(e.movement_target.is_some());
        assert!(e.navigation.nav_com.is_none());
        let mut expected = queue;
        expected.clear_live_head();
        assert_eq!(e.navigation.path_replay, expected);
        assert_eq!(e.navigation.nav_queue, queued);
        for _ in 0..80 {
            walk_frame(&mut sim, &rules);
            if sim
                .substrate
                .entities
                .get(actor)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .step_head()
                .is_none()
            {
                break;
            }
        }
        let e = sim.substrate.entities.get(actor).unwrap();
        assert!(
            e.locomotor.as_ref().unwrap().step_head().is_none(),
            "null destination still finishes its paid step"
        );
        assert_eq!(
            (e.position.rx, e.position.ry),
            ((head.x / 256) as u16, (head.y / 256) as u16)
        );
        assert_eq!(e.navigation.nav_queue, queued);
    }
}

#[test]
fn walk_null_setter_matches_original_caller_rows() {
    use crate::sim::components::{DriveCoord, FootPathQueue, MovementTarget, NavTargetRef};
    use crate::sim::mission::{MissionDispatchTimer, MissionId, state::MissionTestFixture};
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/walk_percell_stop.json"
    ))
    .unwrap();
    for row in corpus
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["setter_only"] == true)
    {
        let input = &row["input"];
        let (mut sim, mut rules, actor, victim) = walk_pursuit_scene();
        rules.general.blockage_path_delay_ticks = 22;
        let e = sim.substrate.entities.get_mut(actor).unwrap();
        e.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(input["mission"].as_i64().unwrap_or(1) as i32),
            queued: MissionId::NONE,
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        e.navigation.nav_com = Some(NavTargetRef::Entity { id: victim });
        e.navigation.nav_com_aux = Some(NavTargetRef::Entity { id: victim });
        e.navigation.path_replay = FootPathQueue {
            directions: vec![2, 3, 4, 5],
            cursor: 0,
            reference_cell: Some((10, 10)),
        };
        if input["nav_queue"] == 1 {
            e.navigation
                .nav_queue
                .push(NavTargetRef::Entity { id: victim });
        }
        if input["contact"] == true {
            e.radio_contacts.insert(victim);
        }
        e.movement_target = Some(MovementTarget::default());
        e.navigation.path_runtime.start_movement(0, 5);
        e.navigation.path_runtime.start_blocked(0, 6);
        e.navigation.path_runtime.path_blocked = true;
        let loco = e.locomotor.as_mut().unwrap();
        loco.set_walk_destination(Some(DriveCoord {
            x: 7808,
            y: 2688,
            z: 0,
        }));
        let head = DriveCoord {
            x: 2880,
            y: 2624,
            z: 0,
        };
        if input["head"] == true || input["process_motion"] == true {
            loco.set_step_head(Some(head));
        }
        if input["process_motion"] == true {
            loco.begin_walk_motion();
            if input["head"] != true {
                loco.set_step_head(None);
            }
        }
        assert!(sim.set_walk_null_destination(actor, Some(&rules)));
        let e = sim.substrate.entities.get(actor).unwrap();
        let loco = e.locomotor.as_ref().unwrap();
        assert_eq!(loco.walk_is_moving(), Some(row["moving"] == 1), "{input}");
        assert_eq!(
            loco.walk_animation_moving(),
            Some(row["animation_moving"] == 1),
            "{input}"
        );
        assert_eq!(
            loco.step_head(),
            if input["head"] == true {
                Some(head)
            } else {
                None
            }
        );
        assert_eq!(loco.walk_destination(), None);
        assert!(e.navigation.nav_com.is_none());
        assert!(e.navigation.nav_com_aux.is_none());
        let expected: Vec<u8> = row["queue"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as u8)
            .collect();
        assert_eq!(e.navigation.path_replay.directions, expected, "{input}");
        assert_eq!(e.navigation.path_replay.reference_cell, Some((10, 10)));
        assert_eq!(
            e.navigation.nav_queue.len(),
            row["nav_queue_count"].as_u64().unwrap() as usize
        );
        assert!(e.movement_target.is_some());
        let path = &e.navigation.path_runtime;
        assert!(!path.path_blocked);
        assert_eq!(
            path.movement_timer
                .remaining(sim.session.binary_frame as i32),
            0
        );
        assert_eq!(
            path.blocked_timer
                .remaining(sim.session.binary_frame as i32),
            22
        );
    }
}
