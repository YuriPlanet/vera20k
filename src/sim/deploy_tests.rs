//! Unit tests for the GI deploy-fire state machine (Slice B1).

#![cfg(test)]

use std::collections::BTreeMap;

use crate::map::bridge_facts::{BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_STRUCTURAL};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
use crate::sim::base_plan::{BasePlanNode, pack_base_plan_cell};
use crate::sim::combat::AttackTarget;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::Health;
use crate::sim::deploy::{DEPLOY_DEFAULT_TICKS, DeployPhase, frames_to_ticks};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseAiActivationLatches;
use crate::sim::world::{SimSoundEvent, Simulation};

const SPLIT_AI_ACTIVATION: HouseAiActivationLatches = HouseAiActivationLatches {
    production: true,
    autocreate_allowed: false,
    ai_triggers_active: false,
    auto_base_building: true,
};

const ENABLED_AI_ACTIVATION: HouseAiActivationLatches = HouseAiActivationLatches {
    production: true,
    autocreate_allowed: false,
    ai_triggers_active: true,
    auto_base_building: true,
};

/// Test ruleset with E1 (DeployFire=yes, GIDeploy/GIUndeploy sounds) and E2
/// (no DeployFire). Mirrors the [InfantryTypes] / [General] / weapon section
/// scaffolding from the canonical fixture in `ruleset.rs::make_test_rules`.
fn make_rules_with_deploy() -> RuleSet {
    let text = "\
[InfantryTypes]
0=E1
1=E2

[General]
BuildSpeed=0.75
MultipleFactory=0.7
LowPowerPenaltyModifier=1.25
MinLowPowerProductionSpeed=0.4
MaxLowPowerProductionSpeed=0.85

[VehicleTypes]

[AircraftTypes]

[BuildingTypes]

[E1]
Name=GI
Cost=200
Strength=125
Armor=none
Speed=4
Primary=M60
DeployFire=yes
DeploySound=GIDeploy
UndeploySound=GIUndeploy
IFVMode=2

[E2]
Name=Conscript
Cost=100
Strength=100
Armor=none
Speed=4
Primary=INTL

[M60]
Damage=25
ROF=20
Range=5
Warhead=SA

[INTL]
Damage=20
ROF=20
Range=5
Warhead=SA

[SA]
Verses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%
CellSpread=0
";
    let ini: IniFile = IniFile::from_str(text);
    RuleSet::from_ini(&ini).expect("test ruleset parse")
}

/// Test ruleset where E1 has DeployFire=yes but no DeploySound/UndeploySound.
fn make_rules_no_sounds() -> RuleSet {
    let text = "\
[InfantryTypes]
0=E1

[General]
BuildSpeed=0.75
MultipleFactory=0.7
LowPowerPenaltyModifier=1.25
MinLowPowerProductionSpeed=0.4
MaxLowPowerProductionSpeed=0.85

[VehicleTypes]

[AircraftTypes]

[BuildingTypes]

[E1]
Name=GI
Cost=200
Strength=125
Armor=none
Speed=4
Primary=M60
DeployFire=yes

[M60]
Damage=25
ROF=20
Range=5
Warhead=SA

[SA]
Verses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%
CellSpread=0
";
    let ini: IniFile = IniFile::from_str(text);
    RuleSet::from_ini(&ini).expect("test ruleset parse")
}

fn make_mcv_rules() -> RuleSet {
    let text = "\
[InfantryTypes]

[VehicleTypes]
0=AMCV
1=SMIN

[AircraftTypes]

[BuildingTypes]
0=GACNST
1=GAPOWR
2=YAREFN

[AMCV]
Name=Allied MCV
Strength=450
Armor=heavy
Speed=5
DeploysInto=GACNST

[SMIN]
Name=Slave Miner
Strength=2000
Armor=heavy
Speed=3
DeploysInto=YAREFN

[GACNST]
Name=Construction Yard
Strength=1000
Armor=wood
Foundation=4x3
ConstructionYard=yes
UndeploysInto=AMCV

[GAPOWR]
Name=Power Plant
Strength=750
Armor=wood
Foundation=1x1

[YAREFN]
Name=Slave Miner Refinery
Strength=1000
Armor=wood
Foundation=2x2
";
    let ini: IniFile = IniFile::from_str(text);
    RuleSet::from_ini(&ini).expect("MCV test ruleset parse")
}

fn make_recalc_mcv_rules(vector_values: &str) -> RuleSet {
    let text = format!(
        "[General]\n\
         HarvesterUnit=HARV\n\
         AISlaveMinerNumber={vector_values}\n\
         AIExtraRefineries={vector_values}\n\
         AlliedBaseDefenseCounts={vector_values}\n\
         SovietBaseDefenseCounts={vector_values}\n\
         ThirdBaseDefenseCounts={vector_values}\n\
         [AI]\n\
         BuildConst=GACNST\nBuildPower=GAPOWR\nBuildRefinery=GAREFN\n\
         BuildBarracks=GAPILE\nBuildWeapons=GAWEAP\nBuildRadar=GAAIRC\nBuildTech=GATECH\n\
         [Countries]\n0=Americans\n[Sides]\nAllied=Americans\n[Americans]\nSide=Allied\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=AMCV\n1=HARV\n2=SMIN\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GACNST\n1=GAPOWR\n2=GAREFN\n3=GAPILE\n4=GAWEAP\n5=GAAIRC\n6=GATECH\n7=YAREFN\n\
         [AMCV]\nStrength=450\nSpeed=5\nDeploysInto=GACNST\n\
         [HARV]\nOwner=Americans\nStrength=100\nSpeed=5\n\
         [SMIN]\nOwner=Americans\nStrength=100\nSpeed=5\nDeploysInto=YAREFN\n\
         [GACNST]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nStrength=1000\nFoundation=4x3\nConstructionYard=yes\n\
         [GAPOWR]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nStrength=750\nFoundation=1x1\n\
         [GAREFN]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nStrength=1000\nFoundation=2x2\n\
         [GAPILE]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nStrength=500\nFoundation=2x2\n\
         [GAWEAP]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nStrength=1000\nFoundation=3x2\n\
         [GAAIRC]\nOwner=Americans\nAIBuildThis=no\nStrength=600\nFoundation=2x2\n\
         [GATECH]\nOwner=Americans\nAIBuildThis=no\nStrength=500\nFoundation=2x2\n\
         [YAREFN]\nOwner=Americans\nStrength=1000\nFoundation=2x2\n"
    );
    RuleSet::from_ini(&IniFile::from_str(&text)).expect("Recalc deploy fixture")
}

fn spawn_infantry(sim: &mut Simulation, type_str: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    let owner_id = sim.interner.intern(owner);
    let type_id = sim.interner.intern(type_str);
    let id = sim.substrate.next_stable_object_id;
    sim.substrate.next_stable_object_id += 1;
    let mut e = GameEntity::new_at_frame_zero_for_test(
        id,
        rx,
        ry,
        0,
        0,
        owner_id,
        Health { current: 125 },
        type_id,
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    // A directly-inserted GameEntity keeps the constructed `in_limbo` byte;
    // production spawns clear it through Reveal, and order admission reads it.
    e.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(e);
    id
}

/// One flat clear-land cell (also reused by `miner_tests` for the resolved
/// terrain `LandType` fixture).
pub(crate) fn clear_terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        rx,
        ry,
        source_tile_index: 0,
        source_sub_tile: 0,
        final_tile_index: 0,
        final_sub_tile: 0,
        is_wood_bridge_repair_tile: false,
        level: 0,
        filled_clear: false,
        tileset_index: Some(0),
        land_type: 0,
        yr_cell_land_type: 0,
        slope_type: 0,
        template_height: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class: TerrainClass::Clear,
        speed_costs: SpeedCostProfile::default(),
        is_water: false,
        is_cliff_like: false,
        height_in_pixels: 0,
        variant: 0,
        is_rough: false,
        is_road: false,
        accepts_smudge: true,
        allows_tiberium: true,
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
        base_terrain_class: TerrainClass::Clear,
        base_speed_costs: SpeedCostProfile::default(),
        build_blocked: false,
        has_bridge_deck: false,
        bridge_walkable: false,
        bridge_transition: false,
        bridge_deck_level: 0,
        bridge_layer: None,
        bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
        tube_index: None,
        radar_left: [0, 0, 0],
        radar_right: [0, 0, 0],
        has_damaged_data: false,
        bridgehead_anchor_class_at_load: None,
    }
}

pub(crate) fn mcv_deploy_terrain_with(
    mut mutate: impl FnMut(&mut ResolvedTerrainCell),
) -> ResolvedTerrainGrid {
    let width = 32;
    let height = 32;
    let mut cells = Vec::with_capacity(width as usize * height as usize);
    for ry in 0..height {
        for rx in 0..width {
            cells.push(clear_terrain_cell(rx, ry));
        }
    }
    let idx = 21usize * width as usize + 20usize;
    mutate(&mut cells[idx]);
    ResolvedTerrainGrid::from_cells(width, height, cells)
}

fn deploy_mcv_with_terrain(terrain: ResolvedTerrainGrid) -> (bool, bool, usize) {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules, &height_map)
        .expect("spawn MCV");
    sim.resolved_terrain = Some(terrain);

    let applied = sim.deploy_mcv(mcv, &rules, &height_map);
    let mcv_remains = sim.substrate.entities.get(mcv).is_some();
    (applied, mcv_remains, sim.sound_events.len())
}

#[test]
fn deploy_mcv_uses_gamemd_large_foundation_origin_offset() {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules, &height_map)
        .expect("spawn MCV");

    let applied = sim.deploy_mcv(mcv, &rules, &height_map);
    assert!(applied, "clear ConYard footprint should deploy");
    // Deferred-delete: apply_command enqueues the consumed MCV; the end-of-tick P9
    // flush (here invoked directly) frees it. Until then it lingers resolvable-Dying.
    sim.flush_pending_delete();
    assert!(
        sim.substrate.entities.get(mcv).is_none(),
        "MCV should be consumed"
    );

    let gacnst_id = sim
        .interner
        .get("GACNST")
        .expect("GACNST should be interned after deploy");
    assert!(
        sim.substrate
            .entities
            .values()
            .any(|e| { e.type_ref == gacnst_id && e.position.rx == 19 && e.position.ry == 21 })
    );
}

/// Deploy and undeploy have to be exact inverses, so an MCV that deploys and
/// undeploys ends up on the cell it started on.
///
/// gamemd steps one cell north-west on deploy and one cell south-east on
/// undeploy, both behind the same `foundation > 2` gate. VERA used to add
/// `width / 2` on the way back, which is `+2` on the 4x4 Construction Yard, so
/// each cycle walked the vehicle one cell south-east — and it compounded, since
/// nothing ever pulled it back. Verified against gamemd 2026-08-05.
#[test]
fn deploy_then_undeploy_returns_the_mcv_to_its_original_cell() {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", true);
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

    let start = (20u16, 22u16);
    let mcv = sim
        .spawn_object(
            "AMCV",
            "Americans",
            start.0,
            start.1,
            128,
            &rules,
            &height_map,
        )
        .expect("spawn MCV");
    assert!(
        sim.deploy_mcv(mcv, &rules, &height_map),
        "clear ConYard footprint should deploy"
    );
    sim.flush_pending_delete();
    // A yard still playing its build-up cannot undeploy, so let it settle.
    tick_n(&mut sim, &rules, 60);

    let gacnst_id = sim.interner.get("GACNST").expect("GACNST interned");
    let yard = sim
        .substrate
        .entities
        .values()
        .find(|e| e.type_ref == gacnst_id)
        .map(|e| e.stable_id)
        .expect("ConYard should exist after deploy");

    assert!(
        sim.apply_command(
            "Americans",
            &Command::UndeployBuilding { entity_id: yard },
            Some(&rules),
            None,
            &height_map,
        ),
        "the yard we just deployed should undeploy"
    );
    // Undeploy runs the build-up animation in reverse and only spawns the
    // vehicle when it finishes, so the cell under test does not exist yet.
    tick_n(&mut sim, &rules, 40);
    sim.flush_pending_delete();

    let amcv_id = sim.interner.get("AMCV").expect("AMCV interned");
    let landed = sim
        .substrate
        .entities
        .values()
        .find(|e| e.type_ref == amcv_id)
        .map(|e| (e.position.rx, e.position.ry))
        .expect("MCV should exist again after undeploy");
    assert_eq!(
        landed, start,
        "deploy/undeploy must round-trip; drifting here compounds every cycle"
    );
}

#[test]
fn deploy_mcv_accepts_mixed_height_clear_foundation() {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    let mut height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    height_map.insert((20, 21), 1);

    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules, &height_map)
        .expect("spawn MCV");

    let applied = sim.deploy_mcv(mcv, &rules, &height_map);
    assert!(
        applied,
        "clear ConYard footprint should deploy even when foundation cells have mixed heights"
    );
    // Deferred-delete: apply_command enqueues the consumed MCV; the end-of-tick P9
    // flush (here invoked directly) frees it. Until then it lingers resolvable-Dying.
    sim.flush_pending_delete();
    assert!(
        sim.substrate.entities.get(mcv).is_none(),
        "MCV should be consumed"
    );

    let gacnst_id = sim
        .interner
        .get("GACNST")
        .expect("GACNST should be interned after deploy");
    assert!(
        sim.substrate
            .entities
            .values()
            .any(|e| { e.type_ref == gacnst_id && e.position.rx == 19 && e.position.ry == 21 }),
        "Construction Yard should spawn at gamemd's deploy foundation origin"
    );
}

#[test]
fn deploy_mcv_rejects_structure_in_rightmost_foundation_column() {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules, &height_map)
        .expect("spawn MCV");
    let blocker = sim
        .spawn_object("GAPOWR", "Soviets", 21, 22, 0, &rules, &height_map)
        .expect("spawn blocker");

    let applied = sim.deploy_mcv(mcv, &rules, &height_map);
    assert!(
        !applied,
        "structure in the deployed foundation footprint must block MCV deploy"
    );
    assert!(
        sim.substrate.entities.get(mcv).is_some(),
        "MCV should remain"
    );
    assert!(
        sim.substrate.entities.get(blocker).is_some(),
        "blocker should remain"
    );
    let americans = sim.interner.intern("Americans");
    assert!(
        sim.sound_events.iter().any(|event| {
            matches!(event, SimSoundEvent::CannotDeployHere { owner } if *owner == americans)
        }),
        "blocked MCV deploy should emit EVA_CannotDeployHere for the command owner"
    );

    if let Some(gacnst_id) = sim.interner.get("GACNST") {
        assert!(
            !sim.substrate
                .entities
                .values()
                .any(|e| e.type_ref == gacnst_id),
            "blocked deploy must not spawn a Construction Yard"
        );
    }
}

#[test]
fn deploy_mcv_waits_for_target_building_deploy_facing() {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", false);
    let owner = sim.interner.get("Americans").unwrap();
    sim.houses.get_mut(&owner).unwrap().ai_activation = SPLIT_AI_ACTIVATION;
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 64, &rules, &height_map)
        .expect("spawn MCV");

    let applied = sim.deploy_mcv(mcv, &rules, &height_map);
    assert!(applied, "misfaced deploy starts the facing turn");
    let entity = sim
        .substrate
        .entities
        .get(mcv)
        .expect("MCV should remain while turning");
    assert_eq!(entity.facing, 64, "turn must not snap at the request");
    assert_eq!(entity.facing_target, Some(0x80));
    assert!(
        sim.interner.get("GACNST").map_or(true, |yard| !sim
            .substrate
            .entities
            .values()
            .any(|e| e.type_ref == yard)),
        "facing gate must run before ConYard creation"
    );
    assert_eq!(sim.houses[&owner].ai_activation, SPLIT_AI_ACTIVATION);
}

#[test]
fn deploy_mcv_uses_deploys_into_building_deploy_facing_override() {
    let ini = IniFile::from_str(
        "\
[InfantryTypes]
[VehicleTypes]
0=AMCV
[AircraftTypes]
[BuildingTypes]
0=GACNST
[AMCV]
Strength=450
Speed=5
DeploysInto=GACNST
[GACNST]
Strength=1000
Foundation=4x3
ConstructionYard=yes
DeployFacing=2
",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules");
    assert_eq!(rules.object("GACNST").unwrap().deploy_facing, 0x40);
    let mut sim = Simulation::new();
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 0x80, &rules, &height_map)
        .expect("spawn MCV");

    assert!(sim.deploy_mcv(mcv, &rules, &height_map));

    let entity = sim
        .substrate
        .entities
        .get(mcv)
        .expect("MCV should remain while turning");
    assert_eq!(entity.facing, 0x80, "turn must not snap at the request");
    assert_eq!(entity.facing_target, Some(0x40));
}

#[test]
fn deploy_mcv_rejects_overlay_blocked_foundation_cell() {
    let terrain = mcv_deploy_terrain_with(|cell| {
        cell.overlay_blocks = true;
    });

    let (applied, mcv_remains, sound_count) = deploy_mcv_with_terrain(terrain);

    assert!(!applied);
    assert!(mcv_remains);
    assert_eq!(sound_count, 1);
}

#[test]
fn deploy_mcv_rejects_sloped_foundation_cell() {
    let terrain = mcv_deploy_terrain_with(|cell| {
        cell.slope_type = 1;
    });

    let (applied, mcv_remains, sound_count) = deploy_mcv_with_terrain(terrain);

    assert!(!applied);
    assert!(mcv_remains);
    assert_eq!(sound_count, 1);
}

#[test]
fn deploy_mcv_rejects_nonbuildable_land_type_foundation_cell() {
    let terrain = mcv_deploy_terrain_with(|cell| {
        cell.land_type = crate::sim::pathfinding::passability::LandType::Water.as_index();
        cell.yr_cell_land_type = cell.land_type;
        cell.base_build_blocked = true;
        cell.build_blocked = true;
        cell.is_water = true;
    });

    let (applied, mcv_remains, sound_count) = deploy_mcv_with_terrain(terrain);

    assert!(!applied);
    assert!(mcv_remains);
    assert_eq!(sound_count, 1);
}

#[test]
fn deploy_mcv_rejects_live_bridge_foundation_cell() {
    let terrain = mcv_deploy_terrain_with(|cell| {
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
    });

    let (applied, mcv_remains, sound_count) = deploy_mcv_with_terrain(terrain);

    assert!(!applied);
    assert!(mcv_remains);
    assert_eq!(sound_count, 1);
}

#[test]
fn deploy_mcv_rejects_pure_0x400_bridge_marker_foundation_cell() {
    let terrain = mcv_deploy_terrain_with(|cell| {
        cell.bridge_facts.raw_flags = BRIDGE_FLAG_DESTROYED_OR_RAMP;
    });

    let (applied, mcv_remains, sound_count) = deploy_mcv_with_terrain(terrain);

    assert!(!applied);
    assert!(mcv_remains);
    assert_eq!(sound_count, 1);
}

fn add_house(sim: &mut Simulation, owner: &str, is_human: bool) {
    let owner_id = sim.interner.intern(owner);
    sim.houses.insert(
        owner_id,
        crate::sim::house_state::HouseState::new(owner_id, 0, Some(owner_id), is_human, 5000, 10),
    );
}

fn deployed_type<'a>(sim: &'a Simulation, type_name: &str) -> &'a GameEntity {
    let type_id = sim.interner.get(type_name).expect("deployed type interned");
    sim.substrate
        .entities
        .values()
        .find(|entity| entity.type_ref == type_id && !entity.dying)
        .expect("deployed target exists")
}

#[test]
fn base_plan_recalc_deploy_generates_and_anchors_nonhuman_conyard() {
    let rules = make_recalc_mcv_rules("0,0,0");
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", false);
    sim.session.game_mode_nonzero = true;
    sim.scenario_rng = crate::sim::rng::SimRng::new(0x1020_3040);
    let height_map = BTreeMap::new();
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules, &height_map)
        .expect("spawn MCV");
    let mut expected_rng = sim.scenario_rng.clone();
    let _replacement_constructor_word = expected_rng.next_u32();

    assert!(sim.deploy_mcv(mcv, &rules, &height_map));

    let yard = deployed_type(&sim, "GACNST");
    assert_eq!((yard.position.rx, yard.position.ry), (19, 21));
    assert!(yard.building_up.is_some());
    let owner = sim.interner.get("Americans").unwrap();
    let house = &sim.houses[&owner];
    assert_eq!(house.base_center, Some((19, 21)));
    assert_eq!(house.base_plan_center, (19, 21));
    assert_eq!(house.ai_activation, ENABLED_AI_ACTIVATION);
    assert!(!house.base_plan.nodes.is_empty());
    assert_eq!(
        house.base_plan.nodes[0].packed_cell,
        pack_base_plan_cell(19, 21)
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(mcv).is_none());
}

#[test]
fn base_plan_recalc_deploy_skips_human_campaign_and_non_conyard_targets() {
    let rules = make_recalc_mcv_rules("0,0,0");
    let height_map = BTreeMap::new();

    for (is_human, game_mode_nonzero) in [(true, true), (false, false)] {
        let mut sim = Simulation::new();
        add_house(&mut sim, "Americans", is_human);
        let owner = sim.interner.get("Americans").unwrap();
        sim.houses.get_mut(&owner).unwrap().ai_activation = SPLIT_AI_ACTIVATION;
        sim.session.game_mode_nonzero = game_mode_nonzero;
        sim.scenario_rng = crate::sim::rng::SimRng::new(0x5566_7788);
        let mcv = sim
            .spawn_object("AMCV", "Americans", 20, 22, 128, &rules, &height_map)
            .expect("spawn MCV");
        let mut expected_rng = sim.scenario_rng.clone();
        let _replacement_constructor_word = expected_rng.next_u32();

        assert!(sim.deploy_mcv(mcv, &rules, &height_map));
        assert!(deployed_type(&sim, "GACNST").building_up.is_some());
        let house = &sim.houses[&owner];
        assert_eq!(house.base_center, None);
        assert_eq!(house.base_plan_center, (0, 0));
        assert!(house.base_plan.nodes.is_empty());
        assert_eq!(house.ai_activation, SPLIT_AI_ACTIVATION);
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
        sim.flush_pending_delete();
        assert!(sim.substrate.entities.get(mcv).is_none());
    }

    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", false);
    let owner = sim.interner.get("Americans").unwrap();
    sim.houses.get_mut(&owner).unwrap().ai_activation = SPLIT_AI_ACTIVATION;
    sim.session.game_mode_nonzero = true;
    sim.scenario_rng = crate::sim::rng::SimRng::new(0x99AA_BBCC);
    let miner = sim
        .spawn_object("SMIN", "Americans", 20, 22, 128, &rules, &height_map)
        .expect("spawn deployable miner");
    let mut expected_rng = sim.scenario_rng.clone();
    let _replacement_constructor_word = expected_rng.next_u32();
    assert!(sim.deploy_mcv(miner, &rules, &height_map));
    assert!(deployed_type(&sim, "YAREFN").building_up.is_some());
    assert_eq!(sim.houses[&owner].base_center, None);
    assert_eq!(sim.houses[&owner].base_plan_center, (0, 0));
    assert!(sim.houses[&owner].base_plan.nodes.is_empty());
    assert_eq!(sim.houses[&owner].ai_activation, SPLIT_AI_ACTIVATION);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(miner).is_none());
}

#[test]
fn base_plan_recalc_deploy_countryless_nonempty_plan_only_reanchors_node_zero() {
    let rules = make_recalc_mcv_rules("");
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", false);
    sim.session.game_mode_nonzero = true;
    sim.scenario_rng = crate::sim::rng::SimRng::new(0x1357_2468);
    let owner = sim.interner.get("Americans").unwrap();
    let house = sim.houses.get_mut(&owner).unwrap();
    house.country = None;
    house.ai_activation = SPLIT_AI_ACTIVATION;
    house.base_plan.nodes = vec![BasePlanNode {
        type_or_control: 4,
        packed_cell: 0xAABB_CCDD,
        filled: true,
        retry_count: -7,
    }];
    let height_map = BTreeMap::new();
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules, &height_map)
        .expect("spawn MCV");
    let mut expected_rng = sim.scenario_rng.clone();
    let _replacement_constructor_word = expected_rng.next_u32();

    assert!(sim.deploy_mcv(mcv, &rules, &height_map));
    assert!(deployed_type(&sim, "GACNST").building_up.is_some());
    let house = &sim.houses[&owner];
    assert_eq!(house.base_center, Some((19, 21)));
    assert_eq!(house.base_plan_center, (19, 21));
    assert_eq!(house.ai_activation, ENABLED_AI_ACTIVATION);
    assert_eq!(house.base_plan.nodes.len(), 1);
    assert_eq!(house.base_plan.nodes[0].type_or_control, 4);
    assert_eq!(
        house.base_plan.nodes[0].packed_cell,
        pack_base_plan_cell(19, 21)
    );
    assert!(house.base_plan.nodes[0].filled);
    assert_eq!(house.base_plan.nodes[0].retry_count, -7);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(mcv).is_none());
}

#[test]
fn base_plan_recalc_deploy_countryless_empty_plan_fails_before_removal() {
    let rules = make_recalc_mcv_rules("0,0,0");
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", false);
    sim.session.game_mode_nonzero = true;
    sim.scenario_rng = crate::sim::rng::SimRng::new(0x2468_1357);
    let owner = sim.interner.get("Americans").unwrap();
    let house = sim.houses.get_mut(&owner).unwrap();
    house.country = None;
    house.ai_activation = SPLIT_AI_ACTIVATION;
    let height_map = BTreeMap::new();
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules, &height_map)
        .expect("spawn MCV");
    let rng_before = sim.scenario_rng.state();

    assert!(!sim.deploy_mcv(mcv, &rules, &height_map));

    assert!(!sim.substrate.entities.get(mcv).unwrap().dying);
    let house = &sim.houses[&owner];
    assert_eq!(house.base_center, None);
    assert_eq!(house.base_plan_center, (0, 0));
    assert!(house.base_plan.nodes.is_empty());
    assert_eq!(house.ai_activation, SPLIT_AI_ACTIVATION);
    assert_eq!(sim.scenario_rng.state(), rng_before);
}

#[test]
fn base_plan_recalc_deploy_failures_preserve_source_rng_plan_and_centers() {
    let valid_rules = make_recalc_mcv_rules("0,0,0");
    let malformed_rules = make_recalc_mcv_rules("");
    let height_map = BTreeMap::new();

    for (rules, add_blocker) in [(&valid_rules, true), (&malformed_rules, false)] {
        let mut sim = Simulation::new();
        add_house(&mut sim, "Americans", false);
        let owner = sim.interner.get("Americans").unwrap();
        sim.houses.get_mut(&owner).unwrap().ai_activation = SPLIT_AI_ACTIVATION;
        sim.session.game_mode_nonzero = true;
        sim.scenario_rng = crate::sim::rng::SimRng::new(0xDEAD_BEEF);
        let mcv = sim
            .spawn_object("AMCV", "Americans", 20, 22, 128, rules, &height_map)
            .expect("spawn MCV");
        if add_blocker {
            sim.spawn_object("GAPOWR", "Blocker", 21, 22, 0, rules, &height_map)
                .expect("spawn footprint blocker");
        }
        let rng_before = sim.scenario_rng.state();

        assert!(!sim.deploy_mcv(mcv, rules, &height_map));
        assert!(!sim.substrate.entities.get(mcv).unwrap().dying);
        let house = &sim.houses[&owner];
        assert_eq!(house.base_center, None);
        assert_eq!(house.base_plan_center, (0, 0));
        assert!(house.base_plan.nodes.is_empty());
        assert_eq!(house.ai_activation, SPLIT_AI_ACTIVATION);
        assert_eq!(sim.scenario_rng.state(), rng_before);
    }
}

#[test]
fn base_plan_recalc_deploy_late_yard_unlimbo_failure_preserves_source_and_plan() {
    let rules = make_recalc_mcv_rules("0,0,0");
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", false);
    let owner = sim.interner.get("Americans").unwrap();
    sim.houses.get_mut(&owner).unwrap().ai_activation = SPLIT_AI_ACTIVATION;
    sim.session.game_mode_nonzero = true;
    sim.scenario_rng = crate::sim::rng::SimRng::new(0xABCD_5750);
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 10,
        off_fc: 0,
        off_100: 0,
        off_104: 10,
        off_108: 10,
    });
    let height_map = BTreeMap::new();
    let mcv = sim
        .spawn_object("AMCV", "Americans", 5, 6, 128, &rules, &height_map)
        .expect("MCV anchor is inside the playfield");
    let mut expected_rng = sim.scenario_rng.clone();
    let _failed_yard_constructor_word = expected_rng.next_u32();

    assert!(!sim.deploy_mcv(mcv, &rules, &height_map));

    let source = sim.substrate.entities.get(mcv).expect("source MCV remains");
    assert!(!source.dying);
    assert!(!source.lifecycle.in_limbo);
    assert!(
        sim.substrate
            .entities
            .values()
            .all(|entity| { sim.interner.resolve(entity.type_ref) != "GACNST" || entity.dying }),
        "the rejected yard constructor leaves no live target"
    );
    let house = &sim.houses[&owner];
    assert_eq!(house.base_center, None);
    assert_eq!(house.base_plan_center, (0, 0));
    assert!(house.base_plan.nodes.is_empty());
    assert_eq!(house.ai_activation, SPLIT_AI_ACTIVATION);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state(),
        "the failed target spends only its verified constructor word, not Recalc draws"
    );
}

#[test]
fn conyard_redeploy_runtime_rejects_when_mcv_redeploy_disabled() {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", true);
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let yard = sim
        .spawn_object("GACNST", "Americans", 19, 21, 0, &rules, &height_map)
        .expect("spawn ConYard");
    sim.session.game_options.mcv_redeploy = false;

    let applied = sim.apply_command(
        "Americans",
        &Command::UndeployBuilding { entity_id: yard },
        Some(&rules),
        None,
        &height_map,
    );

    assert!(!applied);
    assert!(
        sim.substrate
            .entities
            .get(yard)
            .unwrap()
            .building_down
            .is_none()
    );
}

#[test]
fn conyard_redeploy_runtime_rejects_non_human_owner() {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", false);
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let yard = sim
        .spawn_object("GACNST", "Americans", 19, 21, 0, &rules, &height_map)
        .expect("spawn ConYard");

    let applied = sim.apply_command(
        "Americans",
        &Command::UndeployBuilding { entity_id: yard },
        Some(&rules),
        None,
        &height_map,
    );

    assert!(!applied);
    assert!(
        sim.substrate
            .entities
            .get(yard)
            .unwrap()
            .building_down
            .is_none()
    );
}

#[test]
fn conyard_redeploy_ui_hides_while_building_queue_busy() {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", true);
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let yard = sim
        .spawn_object("GACNST", "Americans", 19, 21, 0, &rules, &height_map)
        .expect("spawn ConYard");

    assert!(sim.should_show_undeploy_building_command(yard, &rules));
    let owner = sim.interner.get("Americans").unwrap();
    let type_id = sim.interner.intern("GAPOWR");
    // P5d: arm a Building build in the registry (the queue-of-record) so the
    // production-busy gate sees an active Building factory. This deliberately
    // bypasses producer validation because the minimal fixture has no Factory
    // flag, but it must still model StartProduction's held Techno constructor.
    let started = sim.production.factory_shadow.test_enqueue_kernel(
        owner,
        crate::sim::production::ProductionCategory::Building,
        type_id,
        1,
        100,
        0,
    );
    assert!(started);
    crate::sim::production::construct_active_factory_fixture(
        &mut sim,
        &rules,
        owner,
        crate::sim::production::ProductionCategory::Building,
        type_id,
    )
    .expect("low-level fixture constructs at StartProduction");

    assert!(!sim.should_show_undeploy_building_command(yard, &rules));
    assert!(
        sim.can_undeploy_building_runtime(yard, &rules),
        "production-busy is a UI visibility gate, not the runtime CanUndeployMCV core gate"
    );
}

#[test]
fn mcv_redeploy_option_does_not_gate_non_conyard_undeploys_into() {
    let ini = IniFile::from_str(
        "\
[InfantryTypes]
[VehicleTypes]
0=SMIN
[AircraftTypes]
[BuildingTypes]
0=YAREFN
[SMIN]
Strength=2000
Speed=3
[YAREFN]
Strength=1000
Foundation=2x2
UndeploysInto=SMIN
",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules");
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", true);
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let refinery = sim
        .spawn_object("YAREFN", "Americans", 19, 21, 0, &rules, &height_map)
        .expect("spawn refinery");
    sim.session.game_options.mcv_redeploy = false;

    assert!(sim.can_undeploy_building_runtime(refinery, &rules));
}

/// Schedule one command for tick N+1 and run a single advance_tick.
fn dispatch(sim: &mut Simulation, _owner: &str, cmd: Command, rules: &RuleSet) {
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let owner_id = sim.interner.intern(_owner);
    let cmds = vec![CommandEnvelope::new(owner_id, sim.session.tick + 1, cmd)];
    sim.advance_tick(&cmds, Some(rules), &height_map, None, None, 22);
}

/// Apply a command directly via `apply_command` (no tick advance, no combat
/// or animation cleanup), returning whether the handler accepted it. This is
/// the cleanest signal for gate tests — gate fires → returns false; gate
/// passes → returns true (or fails downstream for unrelated reasons, e.g.
/// missing path_grid for Move). For deploy gate tests, the only thing the
/// gate cares about is that the early-return short-circuits the handler.
fn apply(sim: &mut Simulation, owner: &str, cmd: &Command, rules: &RuleSet) -> bool {
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    sim.apply_command(owner, cmd, Some(rules), None, &height_map)
}

fn tick_n(sim: &mut Simulation, rules: &RuleSet, n: u32) {
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    for _ in 0..n {
        sim.advance_tick(&[], Some(rules), &height_map, None, None, 22);
    }
}

#[test]
fn deploy_phase_advances_to_deployed() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);

    dispatch(
        &mut sim,
        "Americans",
        Command::ToggleInfantryDeploy { entity_id: gi },
        &rules,
    );
    assert!(matches!(
        sim.substrate.entities.get(gi).unwrap().deploy_state,
        Some(DeployPhase::Deploying { .. })
    ));

    let n = DEPLOY_DEFAULT_TICKS as u32;
    tick_n(&mut sim, &rules, n);
    assert_eq!(
        sim.substrate.entities.get(gi).unwrap().deploy_state,
        Some(DeployPhase::Deployed)
    );
}

#[test]
fn undeploy_phase_clears_to_none() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);
    sim.substrate.entities.get_mut(gi).unwrap().deploy_state = Some(DeployPhase::Deployed);

    dispatch(
        &mut sim,
        "Americans",
        Command::ToggleInfantryDeploy { entity_id: gi },
        &rules,
    );
    assert!(matches!(
        sim.substrate.entities.get(gi).unwrap().deploy_state,
        Some(DeployPhase::Undeploying { .. })
    ));

    let n = DEPLOY_DEFAULT_TICKS as u32;
    tick_n(&mut sim, &rules, n);
    assert_eq!(sim.substrate.entities.get(gi).unwrap().deploy_state, None);
}

#[test]
fn mid_deploying_toggle_ignored() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);
    sim.substrate.entities.get_mut(gi).unwrap().deploy_state =
        Some(DeployPhase::Deploying { ticks_remaining: 3 });

    let sounds_before = sim.sound_events.len();
    dispatch(
        &mut sim,
        "Americans",
        Command::ToggleInfantryDeploy { entity_id: gi },
        &rules,
    );
    // Tick advance still runs; Deploying decremented from 3 → 2 (or to Deployed if already 1).
    assert!(matches!(
        sim.substrate.entities.get(gi).unwrap().deploy_state,
        Some(DeployPhase::Deploying { .. }) | Some(DeployPhase::Deployed)
    ));
    let new_deploy_undeploy_sounds = sim
        .sound_events
        .iter()
        .skip(sounds_before)
        .filter(|e| {
            matches!(
                e,
                SimSoundEvent::EntityDeployed { .. } | SimSoundEvent::EntityUndeployed { .. }
            )
        })
        .count();
    assert_eq!(new_deploy_undeploy_sounds, 0);
}

#[test]
fn mid_undeploying_toggle_ignored() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);
    sim.substrate.entities.get_mut(gi).unwrap().deploy_state =
        Some(DeployPhase::Undeploying { ticks_remaining: 3 });

    dispatch(
        &mut sim,
        "Americans",
        Command::ToggleInfantryDeploy { entity_id: gi },
        &rules,
    );
    assert!(matches!(
        sim.substrate.entities.get(gi).unwrap().deploy_state,
        Some(DeployPhase::Undeploying { .. }) | None
    ));
}

#[test]
fn move_silently_ignored_on_deployed() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);
    sim.substrate.entities.get_mut(gi).unwrap().deploy_state = Some(DeployPhase::Deployed);

    let applied = apply(
        &mut sim,
        "Americans",
        &Command::Move {
            entity_id: gi,
            target_rx: 30,
            target_ry: 30,
            queue: false,
            group_id: None,
        },
        &rules,
    );
    assert!(!applied, "gate must reject Move on deployed unit");
    let entity = sim.substrate.entities.get(gi).unwrap();
    assert!(entity.movement_target.is_none());
}

#[test]
fn move_silently_ignored_on_deploying() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);
    sim.substrate.entities.get_mut(gi).unwrap().deploy_state =
        Some(DeployPhase::Deploying { ticks_remaining: 5 });

    let applied = apply(
        &mut sim,
        "Americans",
        &Command::Move {
            entity_id: gi,
            target_rx: 30,
            target_ry: 30,
            queue: false,
            group_id: None,
        },
        &rules,
    );
    assert!(!applied, "gate must reject Move on Deploying unit");
}

#[test]
fn move_silently_ignored_on_undeploying() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);
    sim.substrate.entities.get_mut(gi).unwrap().deploy_state =
        Some(DeployPhase::Undeploying { ticks_remaining: 5 });

    let applied = apply(
        &mut sim,
        "Americans",
        &Command::Move {
            entity_id: gi,
            target_rx: 30,
            target_ry: 30,
            queue: false,
            group_id: None,
        },
        &rules,
    );
    assert!(!applied, "gate must reject Move on Undeploying unit");
}

#[test]
fn attack_move_silently_ignored_on_deployed() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);
    sim.substrate.entities.get_mut(gi).unwrap().deploy_state = Some(DeployPhase::Deployed);

    let applied = apply(
        &mut sim,
        "Americans",
        &Command::AttackMove {
            entity_id: gi,
            target_rx: 30,
            target_ry: 30,
            queue: false,
        },
        &rules,
    );
    assert!(!applied, "gate must reject AttackMove on deployed unit");
    let entity = sim.substrate.entities.get(gi).unwrap();
    assert!(entity.movement_target.is_none());
    assert!(entity.order_intent.is_none());
}

#[test]
fn enter_transport_silently_ignored_on_deployed() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);
    sim.substrate.entities.get_mut(gi).unwrap().deploy_state = Some(DeployPhase::Deployed);

    let applied = apply(
        &mut sim,
        "Americans",
        &Command::EnterTransport {
            passenger_id: gi,
            transport_id: 9999,
        },
        &rules,
    );
    assert!(!applied, "gate must reject EnterTransport on deployed unit");
    assert!(matches!(
        sim.substrate.entities.get(gi).unwrap().passenger_role,
        crate::sim::passenger::PassengerRole::None
    ));
}

#[test]
fn move_works_after_undeploy_completes() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);
    sim.substrate.entities.get_mut(gi).unwrap().deploy_state = Some(DeployPhase::Deployed);

    // Apply ToggleInfantryDeploy directly — gate doesn't apply to it (it's
    // the toggle itself), and the handler returns true on Deployed → Undeploying.
    let toggled = apply(
        &mut sim,
        "Americans",
        &Command::ToggleInfantryDeploy { entity_id: gi },
        &rules,
    );
    assert!(toggled);
    assert!(matches!(
        sim.substrate.entities.get(gi).unwrap().deploy_state,
        Some(DeployPhase::Undeploying { .. })
    ));

    let n = DEPLOY_DEFAULT_TICKS as u32;
    tick_n(&mut sim, &rules, n);
    assert_eq!(sim.substrate.entities.get(gi).unwrap().deploy_state, None);

    // Now the gate must let Move through — it'll fail downstream for
    // unrelated reasons (no path_grid), but only AFTER the gate.
    // The test signal is: the gate doesn't fire (we don't get the gate's
    // early `false` return). Since Move with no path_grid also returns
    // `false` past the gate, we instead check via dock_state — Move
    // mutates `e.dock_state = None` BEFORE the path_grid check; if the
    // gate fired, dock_state would stay Some.
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    sim.substrate.entities.get_mut(gi).unwrap().dock_state =
        Some(crate::sim::docking::building_dock::DockState {
            dock_building_id: 9999,
            phase: crate::sim::docking::building_dock::DockPhase::Approach,
            service_timer: 0,
            no_funds_ticks: 0,
            enter_retry: Default::default(),
        });
    let _ = sim.apply_command(
        "Americans",
        &Command::Move {
            entity_id: gi,
            target_rx: 12,
            target_ry: 12,
            queue: false,
            group_id: None,
        },
        Some(&rules),
        None,
        &height_map,
    );
    assert!(
        sim.substrate.entities.get(gi).unwrap().dock_state.is_none(),
        "Move handler past the gate must clear dock_state after undeploy completes"
    );
}

#[test]
fn deploy_sound_emits_alongside_state_write() {
    // Regression lock for the emit-before-state-write reorder: both effects
    // (sound buffered + deploy_state = Deploying) must be observable after a
    // single ToggleInfantryDeploy command.
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 25, 30);

    assert!(
        sim.substrate
            .entities
            .get(gi)
            .unwrap()
            .deploy_state
            .is_none()
    );
    let events_before = sim.sound_events.len();

    let applied = apply(
        &mut sim,
        "Americans",
        &Command::ToggleInfantryDeploy { entity_id: gi },
        &rules,
    );
    assert!(applied);

    let entity = sim.substrate.entities.get(gi).unwrap();
    assert!(matches!(
        entity.deploy_state,
        Some(DeployPhase::Deploying { .. })
    ));
    assert_eq!(sim.sound_events.len(), events_before + 1);
    assert!(matches!(
        sim.sound_events.last().unwrap(),
        SimSoundEvent::EntityDeployed { .. }
    ));
}

#[test]
fn deploy_sound_emitted_on_phase_entry() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 25, 30);

    dispatch(
        &mut sim,
        "Americans",
        Command::ToggleInfantryDeploy { entity_id: gi },
        &rules,
    );
    let evs: Vec<_> = sim
        .sound_events
        .iter()
        .filter_map(|e| match e {
            SimSoundEvent::EntityDeployed {
                deploy_sound_id,
                rx,
                ry,
            } => Some((*deploy_sound_id, *rx, *ry)),
            _ => None,
        })
        .collect();
    assert_eq!(evs.len(), 1);
    let (id, rx, ry) = evs[0];
    assert_eq!(sim.interner.resolve(id), "GIDeploy");
    assert_eq!((rx, ry), (25, 30));
}

#[test]
fn deploy_sound_suppressed_when_unset() {
    let rules = make_rules_no_sounds();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 25, 30);

    dispatch(
        &mut sim,
        "Americans",
        Command::ToggleInfantryDeploy { entity_id: gi },
        &rules,
    );
    let count = sim
        .sound_events
        .iter()
        .filter(|e| matches!(e, SimSoundEvent::EntityDeployed { .. }))
        .count();
    assert_eq!(count, 0);
    assert!(matches!(
        sim.substrate.entities.get(gi).unwrap().deploy_state,
        Some(DeployPhase::Deploying { .. })
    ));
}

#[test]
fn undeploy_sound_emitted_on_phase_entry() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 25, 30);
    sim.substrate.entities.get_mut(gi).unwrap().deploy_state = Some(DeployPhase::Deployed);

    dispatch(
        &mut sim,
        "Americans",
        Command::ToggleInfantryDeploy { entity_id: gi },
        &rules,
    );
    let count = sim
        .sound_events
        .iter()
        .filter(|e| matches!(e, SimSoundEvent::EntityUndeployed { .. }))
        .count();
    assert_eq!(count, 1);
}

#[test]
fn non_deploy_fire_infantry_no_op() {
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let conscript = spawn_infantry(&mut sim, "E2", "Soviets", 10, 10);

    dispatch(
        &mut sim,
        "Soviets",
        Command::ToggleInfantryDeploy {
            entity_id: conscript,
        },
        &rules,
    );
    assert!(
        sim.substrate
            .entities
            .get(conscript)
            .unwrap()
            .deploy_state
            .is_none()
    );
}

#[test]
fn hash_deterministic_through_full_cycle() {
    let rules = make_rules_with_deploy();
    let mut sim_a = Simulation::new();
    let mut sim_b = Simulation::new();
    let gi_a = spawn_infantry(&mut sim_a, "E1", "Americans", 10, 10);
    let gi_b = spawn_infantry(&mut sim_b, "E1", "Americans", 10, 10);
    assert_eq!(gi_a, gi_b);

    for _ in 0..3 {
        dispatch(
            &mut sim_a,
            "Americans",
            Command::ToggleInfantryDeploy { entity_id: gi_a },
            &rules,
        );
        dispatch(
            &mut sim_b,
            "Americans",
            Command::ToggleInfantryDeploy { entity_id: gi_b },
            &rules,
        );
        let n = DEPLOY_DEFAULT_TICKS as u32;
        for _ in 0..n {
            tick_n(&mut sim_a, &rules, 1);
            tick_n(&mut sim_b, &rules, 1);
            assert_eq!(sim_a.state_hash(), sim_b.state_hash());
        }
    }
}

#[test]
fn snapshot_round_trip_mid_deploying() {
    use crate::sim::snapshot::GameSnapshot;
    let _rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);
    sim.substrate.entities.get_mut(gi).unwrap().deploy_state =
        Some(DeployPhase::Deploying { ticks_remaining: 5 });

    let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let snap = GameSnapshot::load(&bytes).expect("load");
    assert_eq!(
        snap.sim.substrate.entities.get(gi).unwrap().deploy_state,
        Some(DeployPhase::Deploying { ticks_remaining: 5 })
    );
}

#[test]
fn combat_fires_during_deployed_attack() {
    use crate::sim::animation::{
        Animation, FacingSlots, LoopMode, SequenceDef, SequenceKind, SequenceSet, tick_animations,
    };

    let _rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);
    sim.substrate.entities.get_mut(gi).unwrap().deploy_state = Some(DeployPhase::Deployed);
    sim.substrate.entities.get_mut(gi).unwrap().attack_target = Some(AttackTarget {
        target: crate::sim::combat::TargetKind::Entity(9999),
        cooldown_ticks: 10,
        burst_delay_ticks: 0,
        pending_infantry_fire: Some(crate::sim::combat::PendingInfantryFire {
            sequence: SequenceKind::DeployedFire,
            fire_frame: 2,
        }),
    });
    sim.substrate.entities.get_mut(gi).unwrap().animation =
        Some(Animation::new(SequenceKind::Deployed));

    let mut sequences: BTreeMap<String, SequenceSet> = BTreeMap::new();
    let mut set = SequenceSet::new();
    set.insert(
        SequenceKind::Deployed,
        SequenceDef {
            start_frame: 0,
            frame_count: 1,
            facings: 8,
            facing_multiplier: 1,
            frame_delay: 1,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::Loop,
            facing_slots: FacingSlots::InfantryTable,
        },
    );
    set.insert(
        SequenceKind::DeployedFire,
        SequenceDef {
            start_frame: 8,
            frame_count: 6,
            facings: 8,
            facing_multiplier: 6,
            frame_delay: 1,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::TransitionTo(SequenceKind::Deployed),
            facing_slots: FacingSlots::InfantryTable,
        },
    );
    sequences.insert("E1".to_string(), set);

    let _ = tick_animations(
        &mut sim.substrate.entities,
        &sequences,
        &crate::sim::game_options::GameOptions::default(),
        &sim.interner,
        0,
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(gi)
            .unwrap()
            .animation
            .as_ref()
            .unwrap()
            .sequence,
        SequenceKind::DeployedFire
    );
}

/// Test ruleset with GGI and a merged art entry that defines
/// GuardianGISequence with Deploy=300,15,0 and Undeploy=180,2,2.
fn make_rules_with_ggi_art() -> RuleSet {
    let rules_text = "\
[InfantryTypes]
0=GGI

[General]
BuildSpeed=0.75
MultipleFactory=0.7
LowPowerPenaltyModifier=1.25
MinLowPowerProductionSpeed=0.4
MaxLowPowerProductionSpeed=0.85

[VehicleTypes]

[AircraftTypes]

[BuildingTypes]

[GGI]
Name=Guardian GI
Cost=400
Strength=100
Armor=none
Speed=4
Primary=M60
DeployFire=yes
DeploySound=GuardianDeploy

[M60]
Damage=15
ROF=20
Range=4
Warhead=SA

[SA]
Verses=100%,80%,80%,50%,25%,25%,75%,50%,25%,100%,100%
CellSpread=0
";
    let rules_ini = IniFile::from_str(rules_text);
    let mut rules = RuleSet::from_ini(&rules_ini).expect("rules parse");
    let art_ini = IniFile::from_str(
        "[GGI]\n\
         Sequence=GuardianGISequence\n\
         \n\
         [GuardianGISequence]\n\
         Ready=0,1,1\n\
         Walk=8,6,6\n\
         Deploy=300,15,0\n\
         Undeploy=180,2,2\n\
         Deployed=315,1,1\n\
         DeployedFire=323,6,6\n",
    );
    let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    rules.merge_art_data(&art);
    rules.art_registry = art;
    rules
}

#[test]
fn ggi_deploy_uses_art_frame_count() {
    // GGI's GuardianGISequence has Deploy=300,15,0 -> 15 frames.
    // Uses apply() so we observe the raw command effect without any
    // native-frame decrement.
    let rules = make_rules_with_ggi_art();
    let mut sim = Simulation::new();
    let ggi = spawn_infantry(&mut sim, "GGI", "Americans", 10, 10);

    let applied = apply(
        &mut sim,
        "Americans",
        &Command::ToggleInfantryDeploy { entity_id: ggi },
        &rules,
    );
    assert!(applied);

    let entity = sim.substrate.entities.get(ggi).unwrap();
    match entity.deploy_state {
        Some(DeployPhase::Deploying { ticks_remaining }) => {
            assert_eq!(ticks_remaining, 15, "GGI deploy uses 15 native frames");
        }
        other => panic!("expected Deploying, got {:?}", other),
    }
}

#[test]
fn ggi_deploy_begins_decrementing_after_the_command_frame() {
    // Full frame path: the object walk runs first, then the EventClass tail
    // writes the art-derived 15-frame countdown. The first decrement is on the
    // following gameplay frame.
    let rules = make_rules_with_ggi_art();
    let mut sim = Simulation::new();
    let ggi = spawn_infantry(&mut sim, "GGI", "Americans", 10, 10);
    let deploy_ticks = frames_to_ticks(15);

    dispatch(
        &mut sim,
        "Americans",
        Command::ToggleInfantryDeploy { entity_id: ggi },
        &rules,
    );

    assert_eq!(
        sim.substrate.entities.get(ggi).unwrap().deploy_state,
        Some(DeployPhase::Deploying {
            ticks_remaining: deploy_ticks
        })
    );
    tick_n(&mut sim, &rules, (deploy_ticks - 1) as u32);
    assert_eq!(
        sim.substrate.entities.get(ggi).unwrap().deploy_state,
        Some(DeployPhase::Deploying { ticks_remaining: 1 })
    );
    tick_n(&mut sim, &rules, 1);
    assert_eq!(
        sim.substrate.entities.get(ggi).unwrap().deploy_state,
        Some(DeployPhase::Deployed)
    );
}

#[test]
fn ggi_undeploy_uses_art_frame_count() {
    // GuardianGISequence Undeploy=180,2,2 -> 2 native frames.
    let rules = make_rules_with_ggi_art();
    let mut sim = Simulation::new();
    let ggi = spawn_infantry(&mut sim, "GGI", "Americans", 10, 10);
    sim.substrate.entities.get_mut(ggi).unwrap().deploy_state = Some(DeployPhase::Deployed);

    let applied = apply(
        &mut sim,
        "Americans",
        &Command::ToggleInfantryDeploy { entity_id: ggi },
        &rules,
    );
    assert!(applied);

    let entity = sim.substrate.entities.get(ggi).unwrap();
    match entity.deploy_state {
        Some(DeployPhase::Undeploying { ticks_remaining }) => {
            assert_eq!(ticks_remaining, 2, "GGI undeploy uses 2 native frames");
        }
        other => panic!("expected Undeploying, got {:?}", other),
    }
}

#[test]
fn ggi_undeploy_begins_decrementing_after_the_command_frame() {
    // GuardianGISequence Undeploy=180,2,2 -> 2 frames. The EventClass tail
    // starts the countdown after the object walk, so it decrements next frame.
    let rules = make_rules_with_ggi_art();
    let mut sim = Simulation::new();
    let ggi = spawn_infantry(&mut sim, "GGI", "Americans", 10, 10);
    sim.substrate.entities.get_mut(ggi).unwrap().deploy_state = Some(DeployPhase::Deployed);
    let undeploy_ticks = frames_to_ticks(2);

    dispatch(
        &mut sim,
        "Americans",
        Command::ToggleInfantryDeploy { entity_id: ggi },
        &rules,
    );

    assert_eq!(
        sim.substrate.entities.get(ggi).unwrap().deploy_state,
        Some(DeployPhase::Undeploying {
            ticks_remaining: undeploy_ticks
        })
    );
    tick_n(&mut sim, &rules, (undeploy_ticks - 1) as u32);
    assert_eq!(
        sim.substrate.entities.get(ggi).unwrap().deploy_state,
        Some(DeployPhase::Undeploying { ticks_remaining: 1 })
    );
    tick_n(&mut sim, &rules, 1);
    assert_eq!(sim.substrate.entities.get(ggi).unwrap().deploy_state, None);
}

#[test]
fn sequence_less_infantry_falls_back_to_default_ticks() {
    // E1 has no art Sequence= -> compute_anim_ticks falls back to
    // the stock 15-frame default.
    let rules = make_rules_with_deploy();
    let mut sim = Simulation::new();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 10, 10);

    let applied = apply(
        &mut sim,
        "Americans",
        &Command::ToggleInfantryDeploy { entity_id: gi },
        &rules,
    );
    assert!(applied);
    let entity = sim.substrate.entities.get(gi).unwrap();
    match entity.deploy_state {
        Some(DeployPhase::Deploying { ticks_remaining }) => {
            assert_eq!(ticks_remaining, DEPLOY_DEFAULT_TICKS);
        }
        other => panic!("expected Deploying, got {:?}", other),
    }
}
