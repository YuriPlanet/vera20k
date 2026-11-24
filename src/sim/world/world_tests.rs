//! Simulation integration tests — exercises the full tick pipeline: entity spawning,
//! movement commands, combat, bridge traversal, ship pathfinding, deploy/undeploy,
//! and multi-system interactions.

use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use std::collections::BTreeMap;

#[path = "navigation_tests.rs"]
mod navigation_tests;

use super::*;
use crate::map::entities::{EntityCategory, MapEntity};
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::tube_facts::{TubeFact, TubeId};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::particle_system_type::ParticleSystemTypeId;
use crate::rules::particle_type::ParticleTypeId;
use crate::rules::ruleset::RuleSet;
use crate::sim::bridge_state::{BridgeDamageEvent, BridgeRuntimeState};
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::FacingClass;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;
use crate::sim::particles::{Particle, ParticleSystem};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::bridge_orchestrator::blow_up_bridge_cell_fallout;
use crate::util::fixed_math::{SIM_HALF, SIM_ONE, SimFixed};
use glam::IVec3;

fn make_test_entity(type_id: &str, category: EntityCategory) -> MapEntity {
    MapEntity {
        owner: "Americans".to_string(),
        type_id: type_id.to_string(),
        health: 256,
        cell_x: 30,
        cell_y: 40,
        facing: 64,
        category,
        sub_cell: 0,
        veterancy: Some(0),
        high: false,
        mission: None,
        recruitable_a: true,
        recruitable_b: true,
        group: -1,
        follows: None,
        structure_columns: Default::default(),
        structure_ai_sellable: false,
        structure_ai_repairable: false,
        structure_online: true,
        trigger_tag: None,
    }
}

pub(crate) fn gsi_04_07_wall_sell_rules(
    first_unsellable: bool,
    with_sound: bool,
) -> (RuleSet, crate::rules::overlay_types::OverlayTypeRegistry) {
    let ini = IniFile::from_str(&format!(
        "[General]\nFixtureOnly=1\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=FIRSTWALL\n1=SECONDWALL\n\
         [OverlayTypes]\n0=DUMMY0\n1=DUMMY1\n2=GAWALL\n\
         [FIRSTWALL]\nWall=yes\nCost=100\nUnsellable={}\nClickRepairable=no\n\
         [SECONDWALL]\nWall=yes\nCost=200\nUnsellable=no\n\
         [GAWALL]\nWall=yes\nStrength=300\n\
         [AudioVisual]\nSellSound={}\n",
        if first_unsellable { "yes" } else { "no" },
        if with_sound { "SellBuilding" } else { "" },
    ));
    let art_ini = IniFile::from_str(
        "[FIRSTWALL]\nToOverlay=GAWALL\n\
         [SECONDWALL]\nToOverlay=GAWALL\n\
         [GAWALL]\nDamageLevels=3\n",
    );
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).expect("wall-sale rules");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art_ini));
    (rules, overlays)
}

pub(crate) fn gsi_04_07_wall_sell_seed_houses(
    sim: &mut Simulation,
) -> (
    crate::sim::intern::InternedId,
    crate::sim::intern::InternedId,
) {
    let wall_owner = sim.interner.intern("WallOwner");
    let receiver = sim.interner.intern("Receiver");
    let mut owner_house =
        crate::sim::house_state::HouseState::new(wall_owner, 0, None, false, 0, 10);
    owner_house.set_player_control_for_test(true);
    sim.houses.register(owner_house);
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            receiver, 1, None, false, 0, 10,
        ));
    (wall_owner, receiver)
}

/// A house whose tracked buildings and on-map units stand for objects the
/// fixture does not construct, in a non-campaign game, where the defeat gate
/// runs.
fn insert_house_with_counts(
    sim: &mut Simulation,
    name: &str,
    buildings: i32,
    units: i32,
) -> crate::sim::intern::InternedId {
    let owner = sim.interner.intern(name);
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10);
    house
        .tracking_mut_for_test()
        .set_buildings_for_test(buildings);
    house
        .tracking_mut_for_test()
        .set_active_units_for_test(units);
    sim.houses.register(house);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    owner
}

/// The house rung's defeat pass, past frame zero as the gate requires.
fn check_defeat_now(sim: &mut Simulation, rules: Option<&RuleSet>) {
    let session = &mut *sim.session;
    session.set_binary_frame_for_test(session.binary_frame().max(1));
    sim.check_defeat(rules, None);
}

fn insert_test_entity_for_owner(
    sim: &mut Simulation,
    stable_id: u64,
    owner: crate::sim::intern::InternedId,
    type_id: &str,
    category: EntityCategory,
) {
    let owner_name = sim.interner.resolve(owner).to_string();
    let mut entity = GameEntity::test_default(stable_id, type_id, &owner_name, 10, 10);
    entity.set_owner_for_test(owner);
    entity.set_type_ref_for_test(sim.interner.intern(type_id));
    entity.set_category_for_test(category);
    sim.substrate.entities.insert(entity);
    // As its construction would: Add_Tracking.
    crate::sim::house_tracking::HouseTracking::register_object(
        &mut sim.substrate,
        &mut sim.houses,
        stable_id,
    );
}

/// Create a CommandEnvelope with a string owner, interning it via the sim's interner.
fn cmd_envelope(
    sim: &Simulation,
    owner: &str,
    execute_tick: u64,
    payload: Command,
) -> CommandEnvelope {
    let owner_id = sim
        .interner
        .get(owner)
        .unwrap_or_else(|| panic!("owner '{}' not interned", owner));
    CommandEnvelope::new(owner_id, execute_tick, payload)
}

/// A player Move order for `entity_id` to the `target` cell.
fn move_command(entity_id: u64, (target_rx, target_ry): (u16, u16)) -> Command {
    Command::Move {
        entity_id,
        target_rx,
        target_ry,
    }
}

/// Every SpeedType at 100%, the open row of the synthetic clear, water and
/// bridgehead cells.
const OPEN_SPEED_COSTS: crate::rules::terrain_rules::SpeedCostProfile =
    crate::rules::terrain_rules::SpeedCostProfile {
        foot: Some(crate::util::native_x87::NativeF32Bits::ONE),
        track: Some(crate::util::native_x87::NativeF32Bits::ONE),
        wheel: Some(crate::util::native_x87::NativeF32Bits::ONE),
        float: Some(crate::util::native_x87::NativeF32Bits::ONE),
        amphibious: Some(crate::util::native_x87::NativeF32Bits::ONE),
        float_beach: Some(crate::util::native_x87::NativeF32Bits::ONE),
        hover: Some(crate::util::native_x87::NativeF32Bits::ONE),
        winged: Some(crate::util::native_x87::NativeF32Bits::ONE),
    };

/// A water terrain grid for ship tests: every cell Water land with the Water
/// zone, ground-blocked and unbuildable as a loaded Water cell is, and every
/// SpeedType row open.
fn water_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
    use crate::map::resolved_terrain::zone_class;
    use crate::rules::terrain_rules::LandType;

    let water = LandType::Water.as_index();
    let cells = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| crate::map::resolved_terrain::ResolvedTerrainCell {
                land_type: water,
                speed_costs: OPEN_SPEED_COSTS,
                zone_type: zone_class::WATER,
                ground_walk_blocked: true,
                base_ground_walk_blocked: true,
                base_build_blocked: true,
                base_land_type: water,
                base_speed_costs: OPEN_SPEED_COSTS,
                ..crate::map::resolved_terrain::test_flat_cell(x, y)
            })
        })
        .collect();
    ResolvedTerrainGrid::from_cells(width, height, cells)
}

fn gsi_04_10_clear_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
    use crate::map::resolved_terrain::zone_class;
    use crate::rules::terrain_rules::LandType;

    let mut terrain = water_terrain(width, height);
    for cell in terrain.cells_mut_for_test() {
        cell.land_type = LandType::Clear.as_index();
        cell.speed_costs = OPEN_SPEED_COSTS;
        cell.ground_walk_blocked = false;
        cell.zone_type = zone_class::GROUND;
        cell.base_ground_walk_blocked = false;
        cell.base_build_blocked = false;
        cell.base_land_type = LandType::Clear.as_index();
        cell.base_speed_costs = OPEN_SPEED_COSTS;
    }
    terrain
}

/// Supply rectangular bounds for synthetic cell-list and lifecycle fixtures.
fn install_rectangular_test_playfield(sim: &mut Simulation, width: u16, height: u16) {
    let span = i32::from(width.max(height));
    *sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -span,
        off_100: -span,
        off_104: span * 2,
        off_108: span * 2,
    });
}

/// The rectangular playfield with Map Size = the fixture grid, which a
/// Drive/Ship Process's Foot precheck reads.
fn install_rectangular_test_map(sim: &mut Simulation, width: u16, height: u16) {
    install_rectangular_test_playfield(sim, width, height);
    let bounds = sim.playfield_bounds.as_mut().unwrap();
    bounds.base = i32::from(width);
    *sim.playfield_size_height = Some(i32::from(height));
}

fn combat_test_rules() -> RuleSet {
    combat_test_rules_with_override("")
}

fn combat_test_rules_with_override(extra: &str) -> RuleSet {
    combat_rules_with_keys("", "", extra)
}

/// [`combat_test_rules`] with the tank on Drive, so an order drives it.
fn driving_combat_test_rules() -> RuleSet {
    combat_rules_with_keys("", "Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n", "")
}

/// [`driving_combat_test_rules`] with E1 walking and animated, as retail
/// `[E1]` is: without `Locomotor=` a type installs its constructor's Teleport
/// seed, and without a death sequence a corpse neither refuses to fire
/// (`0x0051C8B8`) nor leaves.
fn walking_combat_test_rules() -> RuleSet {
    let art = IniFile::from_str(
        "[GACNST]\nFoundation=4x3\n\
         [E1]\nSequence=E1Sequence\n\
         [E1Sequence]\nReady=0,1,1\nGuard=0,1,1\nProne=86,1,6\nWalk=8,6,6\n\
         FireUp=164,6,6\nFireProne=212,6,6\nDie1=56,15,0\n",
    );
    // ReadSequenceData523D00 reads the sequence in the rules' ART pass, so this
    // ART goes to that pass and is then installed, as the scenario loader does.
    let mut rules = combat_rules_with_art(
        "Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n",
        "Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
        "",
        &art,
    );
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules
}

fn combat_rules_with_keys(infantry_keys: &str, tank_keys: &str, extra: &str) -> RuleSet {
    combat_rules_with_art(
        infantry_keys,
        tank_keys,
        extra,
        &IniFile::from_str("[GACNST]\nFoundation=4x3\n"),
    )
}

fn combat_rules_with_art(
    infantry_keys: &str,
    tank_keys: &str,
    extra: &str,
    art: &IniFile,
) -> RuleSet {
    // Both warheads take the Die1 arm (`InfDeath=1`, retail [SA]'s): the
    // lethal-hit tests keep their victim's corpse through its die sequence.
    let mut ini: IniFile = IniFile::from_str(&format!(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=MTNK\n1=AMCV\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GACNST\n\n\
         [E1]\n{infantry_keys}Strength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [MTNK]\n{tank_keys}Strength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [AMCV]\nStrength=450\nArmor=heavy\nSpeed=5\nPrimary=none\nDeploysInto=GACNST\n\n\
         [GACNST]\nStrength=1000\nArmor=wood\nConstructionYard=yes\nUndeploysInto=AMCV\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\nInfDeath=1\n\n\
         [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\nInfDeath=1\n\
         [Clear]\nBuildable=yes\n",
    ));
    ini.merge(&IniFile::from_str(extra));
    RuleSet::from_ini_with_fixed_art_for_test(&ini, art).expect("combat test rules should parse")
}

/// The retained Drive class state of `entity`'s selected Drive runtime.
fn retained_drive(entity: &GameEntity) -> Option<&DriveLocomotionRuntime> {
    entity
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
}

// The Phase D Task 16 bridge-atlas integration test lives in
// `src/app/presentation/instances/bridges.rs` because it imports render-layer types
// (`BridgeAtlasLookup`, `OverlaySpriteEntry`, `SpriteInstance`) — sim/
// must never depend on render/.

// --- G7 bridgehead registration ---

fn bridgehead_base_cell(rx: u16, ry: u16) -> crate::map::resolved_terrain::ResolvedTerrainCell {
    crate::map::resolved_terrain::ResolvedTerrainCell {
        speed_costs: OPEN_SPEED_COSTS,
        base_speed_costs: OPEN_SPEED_COSTS,
        ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
    }
}

#[path = "world_tests/attack_orders_tests.rs"]
mod attack_orders_tests;
#[path = "world_tests/bridges_tests.rs"]
mod bridges_tests;
#[path = "world_tests/death_lifecycle_tests.rs"]
mod death_lifecycle_tests;
#[path = "world_tests/frame_pipeline_tests.rs"]
mod frame_pipeline_tests;
#[path = "world_tests/group_movement_tests.rs"]
mod group_movement_tests;
#[path = "world_tests/house_outcomes_tests.rs"]
mod house_outcomes_tests;
#[path = "world_tests/terrain_navigation_tests.rs"]
mod terrain_navigation_tests;
#[path = "world_tests/unit_orders_tests.rs"]
mod unit_orders_tests;
#[path = "world_tests/unit_presentation_tests.rs"]
mod unit_presentation_tests;
#[path = "world_tests/wall_sale_tests.rs"]
mod wall_sale_tests;
#[path = "world_tests/weapons_and_waves_tests.rs"]
mod weapons_and_waves_tests;
