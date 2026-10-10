//! Native deployment/Stop receivers and separate Unit/MCV regressions.

#![cfg(test)]

use crate::map::bridge_facts::{BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_STRUCTURAL};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::base_plan::{BasePlanNode, pack_base_plan_cell};
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::command::Command;
use crate::sim::components::Health;
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

fn tick_n(sim: &mut Simulation, rules: &RuleSet, n: u32) {
    for _ in 0..n {
        sim.advance_tick(&[], Some(rules), None, None, 22);
    }
}

const MCV_ART: &str = "[GACNST]\nFoundation=4x3\n\
    [GAPOWR]\nFoundation=1x1\n[YAREFN]\nFoundation=2x2\n\
    [GAREFN]\nFoundation=2x2\n[GAPILE]\nFoundation=2x2\n\
    [GAWEAP]\nFoundation=3x2\n[GAAIRC]\nFoundation=2x2\n[GATECH]\nFoundation=2x2\n";

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
ConstructionYard=yes
UndeploysInto=AMCV

[GAPOWR]
Name=Power Plant
Strength=750
Armor=wood

[YAREFN]
Name=Slave Miner Refinery
Strength=1000
Armor=wood

[Clear]
Buildable=yes
";
    let ini: IniFile = IniFile::from_str(text);
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str(MCV_ART))
        .expect("MCV test ruleset parse")
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
         [GACNST]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nStrength=1000\nConstructionYard=yes\n\
         [GAPOWR]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nStrength=750\n\
         [GAREFN]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nStrength=1000\n\
         [GAPILE]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nStrength=500\n\
         [GAWEAP]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nStrength=1000\n\
         [GAAIRC]\nOwner=Americans\nAIBuildThis=no\nStrength=600\n\
         [GATECH]\nOwner=Americans\nAIBuildThis=no\nStrength=500\n\
         [YAREFN]\nOwner=Americans\nStrength=1000\n\
         [Clear]\nBuildable=yes\n"
    );
    RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(&text),
        &IniFile::from_str(MCV_ART),
    )
    .expect("Recalc deploy fixture")
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

pub(crate) fn mcv_deploy_terrain_with(
    mut mutate: impl FnMut(&mut ResolvedTerrainCell),
) -> ResolvedTerrainGrid {
    crate::map::resolved_terrain::test_grid(32, 32, |rx, ry| {
        let mut cell = crate::map::resolved_terrain::test_tiberium_cell(rx, ry);
        if (rx, ry) == (20, 21) {
            mutate(&mut cell);
        }
        cell
    })
}

/// A Simulation on the shared flat arena, whose ground a Construction Yard
/// may stand on.
fn deploy_sim(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim
}

fn deploy_mcv_with_terrain(terrain: ResolvedTerrainGrid) -> (bool, bool, usize) {
    deploy_mcv_on(terrain, |_| {})
}

fn deploy_mcv_on(
    terrain: ResolvedTerrainGrid,
    prepare: impl FnOnce(&mut Simulation),
) -> (bool, bool, usize) {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
        .expect("spawn MCV");
    sim.resolved_terrain = Some(terrain);
    sim.playfield_bounds = Some(crate::sim::arena_fixture::OPEN_PLAYFIELD);
    prepare(&mut sim);

    let applied = sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let mcv_remains = sim.substrate.entities.get(mcv).is_some();
    (applied, mcv_remains, sim.sound_events.len())
}

#[test]
fn deploy_mcv_uses_gamemd_large_foundation_origin_offset() {
    let rules = make_mcv_rules();
    let mut sim = deploy_sim(&rules);

    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
        .expect("spawn MCV");

    let applied = sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
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
    let mut rules = make_mcv_rules();
    // A yard needs its Buildup SHP to undeploy (`Sell_Back @ 0x00447110`)
    // and converts back only in a multiplayer game (`0x00449D08`).
    rules.set_buildup_control_for_test("GACNST", [0, 29, 1]);
    let mut sim = deploy_sim(&rules);
    sim.session.game_mode_nonzero = true;
    add_house(&mut sim, "Americans", true);

    let start = (20u16, 22u16);
    let mcv = sim
        .spawn_object("AMCV", "Americans", start.0, start.1, 128, &rules)
        .expect("spawn MCV");
    assert!(
        sim.deploy_mcv(
            mcv,
            &rules,
            None,
            crate::sim::world::FrameEffects::default()
        ),
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
    let mut sim = deploy_sim(&rules);
    sim.resolved_terrain
        .as_mut()
        .expect("deploy fixture terrain")
        .cell_mut(20, 21)
        .expect("foundation cell")
        .level = 1;

    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
        .expect("spawn MCV");

    let applied = sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
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
    let mut sim = deploy_sim(&rules);

    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
        .expect("spawn MCV");
    let blocker = sim
        .spawn_object("GAPOWR", "Soviets", 21, 22, 0, &rules)
        .expect("spawn blocker");

    let applied = sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
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
    let mut sim = deploy_sim(&rules);
    add_house(&mut sim, "Americans", false);
    let owner = sim.interner.get("Americans").unwrap();
    sim.houses.get_mut(&owner).unwrap().ai_activation = SPLIT_AI_ACTIVATION;

    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 64, &rules)
        .expect("spawn MCV");

    let applied = sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(applied, "misfaced deploy starts the facing turn");
    let entity = sim
        .substrate
        .entities
        .get(mcv)
        .expect("MCV should remain while turning");
    // Do_Turn toward the yard's DeployFacing; the deploy waits for the turn.
    assert_eq!(entity.body_facing.destination(), 0x8000);
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
ConstructionYard=yes
DeployFacing=2
[Clear]
Buildable=yes
",
    );
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str(MCV_ART))
        .expect("rules");
    assert_eq!(rules.object("GACNST").unwrap().deploy_facing, 0x40);
    let mut sim = deploy_sim(&rules);
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 0x80, &rules)
        .expect("spawn MCV");

    assert!(sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));

    let entity = sim
        .substrate
        .entities
        .get(mcv)
        .expect("MCV should remain while turning");
    assert_eq!(entity.body_facing.destination(), 0x4000);
}

#[test]
fn deploy_mcv_rejects_overlay_blocked_foundation_cell() {
    let terrain = mcv_deploy_terrain_with(|_| {});

    let (applied, mcv_remains, sound_count) = deploy_mcv_on(terrain, |sim| {
        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(32, 32);
        overlays.place_overlay(20, 21, 1, 0);
        sim.overlay_grid = Some(overlays);
    });

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
    let mut sim = deploy_sim(&rules);
    add_house(&mut sim, "Americans", false);
    sim.session.game_mode_nonzero = true;
    sim.scenario_rng = crate::sim::rng::SimRng::new(0x1020_3040);
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
        .expect("spawn MCV");
    let mut expected_rng = sim.scenario_rng.clone();
    let _replacement_constructor_word = expected_rng.next_u32();

    assert!(sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));

    let yard = deployed_type(&sim, "GACNST");
    assert_eq!((yard.position.rx, yard.position.ry), (19, 21));
    assert!(yard.building_up());
    assert!(yard.ai_repairable, "a computer's yard is AI-repairable");
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

/// A computer's damaged MCV deploys into a damaged yard, which starts its
/// repair on the frame its build-up completes, not during it: native
/// Get_Mission already reads the Guard that Grand_Opening queued there
/// (`tools/spatial_oracle/building_repair.json` `build` rows). Through
/// `advance_tick`, the yard's object visit (`production::update_repair_and_power`)
/// runs before the late build-up step completes it.
#[test]
fn a_damaged_computer_yard_starts_its_repair_as_its_build_up_completes() {
    let mut rules = make_recalc_mcv_rules("0,0,0");
    rules.set_buildup_control_for_test("GACNST", [0, 3, 2]);
    let mut sim = deploy_sim(&rules);
    sim.session.game_mode_nonzero = true;
    add_house(&mut sim, "Americans", false);
    let owner = sim.interner.get("Americans").unwrap();
    sim.houses.get_mut(&owner).unwrap().current_iq = 5;
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
        .expect("spawn MCV");
    sim.substrate.entities.get_mut(mcv).unwrap().health.current = 225;
    let deployed_at = sim.session.binary_frame;
    assert!(sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    sim.flush_pending_delete();
    let yard = deployed_type(&sim, "GACNST").stable_id;
    let mut frames = Vec::new();
    for _ in 0..7 {
        let frame = sim.session.binary_frame - deployed_at;
        tick_n(&mut sim, &rules, 1);
        let entity = sim.substrate.entities.get(yard).unwrap();
        let house = &sim.houses[&owner];
        frames.push((
            frame,
            entity.building_up(),
            entity.repairing,
            house.repair_start_latch,
        ));
    }
    // A deploy's 3x2 build-up completes at D + 1 + (3 - 1) * 2 (the
    // `building_construction.json` `route` rows).
    let expected: Vec<_> = (0..7)
        .map(|frame| (frame, frame < 5, frame >= 5, frame >= 5))
        .collect();
    assert_eq!(frames, expected);
    assert_eq!(
        sim.houses[&owner].repair_latch_timer.start_frame(),
        (deployed_at + 5) as i32,
        "the latch timer starts on the completion frame"
    );
}

#[test]
fn base_plan_recalc_deploy_skips_human_campaign_and_non_conyard_targets() {
    let rules = make_recalc_mcv_rules("0,0,0");

    for (is_human, game_mode_nonzero) in [(true, true), (false, false)] {
        let mut sim = deploy_sim(&rules);
        add_house(&mut sim, "Americans", is_human);
        let owner = sim.interner.get("Americans").unwrap();
        sim.houses.get_mut(&owner).unwrap().ai_activation = SPLIT_AI_ACTIVATION;
        sim.session.game_mode_nonzero = game_mode_nonzero;
        sim.scenario_rng = crate::sim::rng::SimRng::new(0x5566_7788);
        let mcv = sim
            .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
            .expect("spawn MCV");
        let mut expected_rng = sim.scenario_rng.clone();
        let _replacement_constructor_word = expected_rng.next_u32();

        assert!(sim.deploy_mcv(
            mcv,
            &rules,
            None,
            crate::sim::world::FrameEffects::default()
        ));
        assert!(deployed_type(&sim, "GACNST").building_up());
        // UnitClass::Deploy marks a computer's building AI-repairable in a
        // campaign too (`0x007397E4..0x007397F4`); Unlimbo does not there.
        assert_eq!(deployed_type(&sim, "GACNST").ai_repairable, !is_human);
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

    let mut sim = deploy_sim(&rules);
    add_house(&mut sim, "Americans", false);
    let owner = sim.interner.get("Americans").unwrap();
    sim.houses.get_mut(&owner).unwrap().ai_activation = SPLIT_AI_ACTIVATION;
    sim.session.game_mode_nonzero = true;
    sim.scenario_rng = crate::sim::rng::SimRng::new(0x99AA_BBCC);
    let miner = sim
        .spawn_object("SMIN", "Americans", 20, 22, 128, &rules)
        .expect("spawn deployable miner");
    let mut expected_rng = sim.scenario_rng.clone();
    let _replacement_constructor_word = expected_rng.next_u32();
    assert!(sim.deploy_mcv(
        miner,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(deployed_type(&sim, "YAREFN").building_up());
    assert!(deployed_type(&sim, "YAREFN").ai_repairable);
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
    let mut sim = deploy_sim(&rules);
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
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
        .expect("spawn MCV");
    let mut expected_rng = sim.scenario_rng.clone();
    let _replacement_constructor_word = expected_rng.next_u32();

    assert!(sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(deployed_type(&sim, "GACNST").building_up());
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
    let mut sim = deploy_sim(&rules);
    add_house(&mut sim, "Americans", false);
    sim.session.game_mode_nonzero = true;
    sim.scenario_rng = crate::sim::rng::SimRng::new(0x2468_1357);
    let owner = sim.interner.get("Americans").unwrap();
    let house = sim.houses.get_mut(&owner).unwrap();
    house.country = None;
    house.ai_activation = SPLIT_AI_ACTIVATION;
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
        .expect("spawn MCV");
    let rng_before = sim.scenario_rng.state();

    assert!(!sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));

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

    for (rules, add_blocker) in [(&valid_rules, true), (&malformed_rules, false)] {
        let mut sim = deploy_sim(rules);
        add_house(&mut sim, "Americans", false);
        let owner = sim.interner.get("Americans").unwrap();
        sim.houses.get_mut(&owner).unwrap().ai_activation = SPLIT_AI_ACTIVATION;
        sim.session.game_mode_nonzero = true;
        sim.scenario_rng = crate::sim::rng::SimRng::new(0xDEAD_BEEF);
        let mcv = sim
            .spawn_object("AMCV", "Americans", 20, 22, 128, rules)
            .expect("spawn MCV");
        if add_blocker {
            sim.spawn_object("GAPOWR", "Blocker", 21, 22, 0, rules)
                .expect("spawn footprint blocker");
        }
        let rng_before = sim.scenario_rng.state();

        assert!(!sim.deploy_mcv(mcv, rules, None, crate::sim::world::FrameEffects::default()));
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
fn conyard_redeploy_runtime_rejects_when_mcv_redeploy_disabled() {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", true);
    let yard = sim
        .spawn_object("GACNST", "Americans", 19, 21, 0, &rules)
        .expect("spawn ConYard");
    sim.session.game_options.mcv_redeploy = false;

    let applied = sim.apply_command(
        "Americans",
        &Command::UndeployBuilding { entity_id: yard },
        Some(&rules),
    );

    assert!(!applied);
    assert!(!sim.substrate.entities.get(yard).unwrap().building_down());
}

#[test]
fn conyard_redeploy_runtime_rejects_non_human_owner() {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", false);
    let yard = sim
        .spawn_object("GACNST", "Americans", 19, 21, 0, &rules)
        .expect("spawn ConYard");

    let applied = sim.apply_command(
        "Americans",
        &Command::UndeployBuilding { entity_id: yard },
        Some(&rules),
    );

    assert!(!applied);
    assert!(!sim.substrate.entities.get(yard).unwrap().building_down());
}

#[test]
fn conyard_redeploy_ui_hides_while_building_queue_busy() {
    let rules = make_mcv_rules();
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", true);
    let yard = sim
        .spawn_object("GACNST", "Americans", 19, 21, 0, &rules)
        .expect("spawn ConYard");

    assert!(sim.should_show_undeploy_building_command(yard, &rules));
    let owner = sim.interner.get("Americans").unwrap();
    let type_id = sim.interner.intern("GAPOWR");
    // P5d: arm a Building build in the registry (the queue-of-record) so the
    // production-busy gate sees an active Building factory. This deliberately
    // bypasses producer validation because the minimal fixture has no Factory
    // flag, but it must still model StartProduction's held Techno constructor.
    let started = sim.production.factories.test_enqueue_kernel(
        owner,
        crate::sim::production::ProductionCategory::Building,
        type_id,
        1,
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
UndeploysInto=SMIN
",
    );
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str(MCV_ART))
        .expect("rules");
    let mut sim = Simulation::new();
    add_house(&mut sim, "Americans", true);
    let refinery = sim
        .spawn_object("YAREFN", "Americans", 19, 21, 0, &rules)
        .expect("spawn refinery");
    sim.session.game_options.mcv_redeploy = false;

    assert!(sim.can_undeploy_building_runtime(refinery, &rules));
}

/// Reproduce only the prior state declared by an original execution row. These
/// authored Type/ART records represent supplied native memory, not retail tuning.
/// The tests exercise the production readers; expected outputs stay in native JSON.
fn native_deploy_rules(input: &serde_json::Value) -> RuleSet {
    let type_name = input["type_id"].as_str().unwrap_or("E1");
    let delays = input["delays"].as_array().map_or_else(
        || "15,25,100".to_owned(),
        |values| {
            values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        },
    );
    let weapon_keys = if input["weapon_present"].as_bool().unwrap_or(true) {
        "Primary=SUPPLIED0\nSecondary=SUPPLIED1\n"
    } else {
        ""
    };
    let idle_keys = input["idle_action_frequency"]
        .as_f64()
        .map(|frequency| format!("[AudioVisual]\nIdleActionFrequency={frequency}\n"))
        .unwrap_or_default();
    let ini = IniFile::from_str(&format!(
        "{idle_keys}[General]\nAIAutoDeployFrameDelay={delays}\n\
         [Guard]\nRate=0.1\n[Sticky]\nRate=0.1\n[Area Guard]\nRate=0.1\n\
         [AI]\nBlockagePathDelay={}\n[InfantryTypes]\n0={type_name}\n\
         [{type_name}]\nStrength=100\nSpeed=4\n\
         Locomotor={{4A582744-9839-11D1-B709-00A024DDAFD1}}\n\
         Deployer={}\nDeployedCrushable={}\nDeployFire={}\nUndeployDelay={}\nImmuneToRadiation={}\n\
         SprayAttack={}\nDeploySound=GIDeploy\nUndeploySound=GIUndeploy\n\
         {weapon_keys}\n\
         [SUPPLIED0]\nAreaFire={}\n[SUPPLIED1]\nAreaFire={}\n",
        input["blockage_path_delay"].as_i64().unwrap_or(0),
        input["deployer"].as_i64().unwrap_or(1),
        input["crushable"].as_i64().unwrap_or(0),
        input["deploy_fire"].as_i64().unwrap_or(1),
        input["undeploy_delay"].as_i64().unwrap_or(-1),
        input["immune"].as_i64().unwrap_or(0),
        input["spray_attack"].as_i64().unwrap_or(0),
        i32::from(
            input["area_fire"].as_i64().unwrap_or(0) != 0
                && input["area_slot"].as_i64().unwrap_or(1) == 0
        ),
        i32::from(
            input["area_fire"].as_i64().unwrap_or(0) != 0
                && input["area_slot"].as_i64().unwrap_or(1) == 1
        ),
    ));
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    let mut counts = [input["count"].as_i64().unwrap_or(6) as i32; 42];
    for record in input["counts"].as_array().into_iter().flatten() {
        counts[record[0].as_u64().unwrap() as usize] = record[1].as_i64().unwrap() as i32;
    }
    let mut text = format!("[{type_name}]\nSequence=SuppliedSequence\n[SuppliedSequence]\n");
    for (name, count) in crate::rules::infantry_sequence::NATIVE_SEQUENCE_NAMES
        .iter()
        .zip(counts)
    {
        text.push_str(&format!("{name}=100,{count},6\n"));
    }
    let art = IniFile::from_str(&text);
    rules.replace_art_registry_for_test(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let sequences = crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art);
    rules.replace_animation_sequences_for_test(
        crate::rules::animation_sequence::build_animation_sequence_catalog(
            &rules,
            Some(&sequences),
        ),
    );
    rules
}

fn native_coord(value: &serde_json::Value) -> Option<crate::sim::components::DriveCoord> {
    let xyz = value.as_array().unwrap();
    let coord = crate::sim::components::DriveCoord {
        x: xyz[0].as_i64().unwrap() as i32,
        y: xyz[1].as_i64().unwrap() as i32,
        z: xyz[2].as_i64().unwrap() as i32,
    };
    (coord.x != 0 || coord.y != 0 || coord.z != 0).then_some(coord)
}

/// Transport a supplied DWORD coordinate into cell origin plus fixed offset.
/// The wide WORD-wrap control exceeds a subcell fixed-point value's range;
/// its full coordinate remains representable with the existing u16 origin.
fn supplied_world_position(
    position: &mut crate::sim::components::Position,
    xyz: &serde_json::Value,
) {
    use crate::util::fixed_math::SimFixed;
    let x = xyz[0].as_i64().unwrap() as i32;
    let y = xyz[1].as_i64().unwrap() as i32;
    position.rx = (x / 256).clamp(0, i32::from(u16::MAX)) as u16;
    position.ry = (y / 256).clamp(0, i32::from(u16::MAX)) as u16;
    position.sub_x = SimFixed::from_num(x - i32::from(position.rx) * 256);
    position.sub_y = SimFixed::from_num(y - i32::from(position.ry) * 256);
    position.exact_z_leptons = Some(xyz[2].as_i64().unwrap() as i32);
}

pub(crate) fn native_deploy_fixture(row: &serde_json::Value) -> (Simulation, RuleSet, u64) {
    use crate::sim::mission::MissionId;
    use crate::util::fixed_math::SimFixed;
    let input = &row["input"];
    let before = &row["before"];
    let rules = native_deploy_rules(input);
    let mut sim = Simulation::with_seed(31);
    sim.mapgen_rng = crate::sim::rng::SimRng::new(31);
    sim.session.binary_frame = input["now"].as_i64().unwrap_or(100) as u32;
    sim.session.game_options.game_speed = input["game_speed_index"].as_i64().unwrap_or(0) as i32;
    let owner = sim.interner.intern("Americans");
    let human = input["human"].as_i64().unwrap_or(0) != 0;
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, human, 0, 10),
    );
    sim.houses.get_mut(&owner).unwrap().set_difficulty(
        crate::sim::house_state::HouseDifficulty::from_native(
            input["difficulty"].as_i64().unwrap_or(1) as i32,
        )
        .unwrap(),
        &rules.general,
        Default::default(),
        sim.session.game_mode_nonzero,
        0,
        0,
    );
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(32, 32, |x, y| {
        crate::map::resolved_terrain::test_tiberium_cell(x, y)
    }));
    let id = spawn_infantry(
        &mut sim,
        input["type_id"].as_str().unwrap_or("E1"),
        "Americans",
        10,
        10,
    );
    sim.mission_assign_exact(
        id,
        MissionId::from_raw(input["mission"].as_i64().unwrap_or(5) as i32),
        input["mission_start"].as_i64().unwrap_or(0) as u32,
    )
    .unwrap();
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.health.current = input["health"].as_i64().unwrap_or(100) as i32;
    actor.position.sub_x = SimFixed::from_num(128);
    actor.position.sub_y = SimFixed::from_num(128);
    actor.position.exact_z_leptons = Some(0);
    if input["position"].is_array() {
        supplied_world_position(&mut actor.position, &input["position"]);
    }
    actor.infantry.as_mut().unwrap().is_prone = before["prone"] != 0;
    if before["idle_timer"].is_array() {
        actor.infantry.as_mut().unwrap().idle_action_timer =
            crate::sim::mission::MissionTimer::armed(
                before["idle_timer"][0].as_i64().unwrap() as u32,
                before["idle_timer"][1].as_i64().unwrap() as u32,
            );
    }
    actor.set_falling_down_for_test(input["falling"].as_i64().unwrap_or(0) != 0);
    actor
        .mission_leaf
        .set_infantry_doing_verified(before["doing"].as_i64().unwrap() as i32)
        .unwrap();
    actor
        .mission_leaf
        .set_infantry_pending_deploy(before["pending"].as_u64().unwrap() as u8);
    actor
        .mission_leaf
        .set_foot_firing_sequence(before["firing"].as_u64().unwrap_or(0) as u8);
    actor.set_infantry_deploy_crush_immunity(before["crush"].as_u64().unwrap() as u8);
    actor.install_native_stage_fixture(crate::sim::stage::StageClass::from_native_fixture(
        before["frame"].as_i64().unwrap() as i32,
        before["changed"].as_u64().unwrap() as u8,
        crate::sim::timer::CdTimer::from_raw(
            before["stage"][0].as_i64().unwrap() as i32,
            before["stage"][1].as_i64().unwrap() as i32,
        ),
        before["stage"][2].as_i64().unwrap() as i32,
        before["stage"][3].as_i64().unwrap() as i32,
    ));
    actor.passive_scan_timer = crate::sim::mission::MissionTimer::armed(
        before["reload"][0].as_i64().unwrap() as u32,
        before["reload"][1].as_i64().unwrap() as u32,
    );
    let mut loco = crate::sim::movement::locomotor::LocomotorState::from_object_type(
        rules
            .object(input["type_id"].as_str().unwrap_or("E1"))
            .unwrap(),
        0,
    );
    // Supplied raw prior bytes are independent: a retained destination need
    // not have moving34 set, and motion36 can survive a paid-head release.
    // MoveTo would invent a transition instead of transporting the fixture.
    loco.runtime_payload =
        crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload::Walk(
            crate::sim::movement::locomotion::piggyback::WalkRuntime {
                head: native_coord(&before["head"]),
                destination: native_coord(&before["destination"]),
                moving: before["moving"] != 0,
                animation_moving: before["motion"] != 0,
            },
        );
    actor.locomotor = Some(loco);
    if before["nav"].as_i64().unwrap_or(0) != 0 {
        actor.navigation.nav_com = Some(crate::sim::components::NavTargetRef::cell(11, 10));
    }
    if before["target"].as_i64().unwrap_or(0) != 0 {
        actor.attack_target = Some(AttackTarget::for_cell(10, 10));
    }
    if input["archive"].is_array() {
        let post = spawn_infantry(&mut sim, "E1", "Americans", 0, 0);
        let entity = sim.substrate.entities.get_mut(post).unwrap();
        supplied_world_position(&mut entity.position, &input["archive"]);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .set_archive_target(Some(TargetKind::Entity(post)));
    }
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        row["rng_before"].as_str().unwrap()
    );
    (sim, rules, id)
}

/// Compare original outputs directly. No Rust predicate or timer/RNG calculation
/// supplies an expected value; ignored native stack residue104 stays outside state.
pub(crate) fn assert_native_deploy_state(sim: &Simulation, id: u64, row: &serde_json::Value) {
    let expected = &row["after"];
    let name = row["input"].to_string();
    let actor = sim.substrate.entities.get(id).unwrap();
    let leaf = actor.mission_leaf.as_infantry().unwrap();
    assert_eq!(
        leaf.doing(),
        expected["doing"].as_i64().unwrap() as i32,
        "{name}: Doing"
    );
    assert_eq!(
        leaf.pending_deploy(),
        expected["pending"].as_u64().unwrap() as u8,
        "{name}: pending6E4"
    );
    assert_eq!(
        actor.native_crush_immunity(),
        expected["crush"].as_u64().unwrap() as u8,
        "{name}: crush2A4"
    );
    assert_eq!(
        actor.infantry.as_ref().unwrap().is_prone,
        expected["prone"] != 0,
        "{name}: prone6DB"
    );
    let stage = actor.native_stage();
    if expected["idle_timer"].is_array() {
        let timer = actor.infantry.as_ref().unwrap().idle_action_timer;
        assert_eq!(
            serde_json::json!([timer.start_frame as i32, timer.duration as i32]),
            expected["idle_timer"],
            "{name}: IdleActionTimer168/170"
        );
    }
    assert_eq!(
        stage.value(),
        expected["frame"].as_i64().unwrap() as i32,
        "{name}: Stage"
    );
    let stage_json = serde_json::to_value(stage).unwrap();
    assert_eq!(
        stage_json["changed"], expected["changed"],
        "{name}: changedFC"
    );
    assert_eq!(
        serde_json::json!([
            stage.timer().start_frame(),
            stage.timer().duration(),
            stage.rate(),
            stage_json["increment"]
        ]),
        expected["stage"],
        "{name}: retained clock",
    );
    assert_eq!(
        serde_json::json!([
            actor.passive_scan_timer.start_frame as i32,
            actor.passive_scan_timer.duration as i32
        ]),
        expected["reload"],
        "{name}: passive targeting timer180/188",
    );
    let loco = actor.locomotor.as_ref().unwrap();
    assert_eq!(
        loco.walk_destination(),
        native_coord(&expected["destination"]),
        "{name}: Walk destination"
    );
    assert_eq!(
        loco.step_head(),
        native_coord(&expected["head"]),
        "{name}: retained paid head"
    );
    assert_eq!(
        loco.walk_is_moving(),
        Some(expected["moving"] != 0),
        "{name}: Walk moving34"
    );
    assert_eq!(
        loco.walk_animation_moving(),
        Some(expected["motion"] != 0),
        "{name}: Walk motion36"
    );
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        row["rng_after"].as_str().unwrap(),
        "{name}: full Scenario RNG"
    );
    if row["rng_streams_after"].is_object() {
        assert_eq!(
            sim.main_rng.native_state_hex(),
            row["rng_streams_after"]["main"].as_str().unwrap(),
            "{name}: full Main RNG"
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            row["rng_streams_after"]["mapgen"].as_str().unwrap(),
            "{name}: full MapGen RNG"
        );
    }
    if expected["mission"].is_number() {
        assert_eq!(
            actor.mission.current().raw(),
            expected["mission"].as_i64().unwrap() as i32,
            "{name}: Assign Guard"
        );
        assert_eq!(
            actor.mission.queued().raw(),
            expected["queued"].as_i64().unwrap() as i32,
            "{name}: queued mission"
        );
        assert_eq!(
            actor.mission.suspended().raw(),
            expected["suspended"].as_i64().unwrap() as i32,
            "{name}: suspended mission"
        );
        assert_eq!(
            actor.mission.handler_state(),
            expected["handler_state"].as_u64().unwrap() as u32,
            "{name}: handler state"
        );
        assert_eq!(
            actor.mission.mission_start_frame() as i32,
            expected["mission_start"].as_i64().unwrap() as i32,
            "{name}: assignment frame"
        );
        assert_eq!(
            actor.navigation.nav_com.is_some(),
            expected["nav"] != 0,
            "{name}: NavCom"
        );
        assert_eq!(
            actor.archive_target().is_some(),
            expected["archive"] != 0,
            "{name}: archive identity retained"
        );
        let target = actor.attack_target.as_ref().map(|target| target.target);
        assert_eq!(
            target,
            (expected["target"] != 0).then_some(TargetKind::Cell(10, 10)),
            "{name}: Cell target"
        );
        assert_eq!(
            leaf.firing_sequence_latch(),
            expected["firing"].as_u64().unwrap() as u8,
            "{name}: firing68D"
        );
    }
    let native_sounds = row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| {
            (event["address"] == "007509E0").then(|| event["sound"].as_u64().unwrap())
        })
        .map(|sound| match sound {
            101 => "GIDeploy",
            102 => "GIUndeploy",
            other => panic!("unknown supplied sound {other}"),
        })
        .collect::<Vec<_>>();
    let sounds = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::EntityDeployed {
                deploy_sound_id, ..
            } => Some(sim.interner.resolve(*deploy_sound_id)),
            SimSoundEvent::EntityUndeployed {
                undeploy_sound_id, ..
            } => Some(sim.interner.resolve(*undeploy_sound_id)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sounds, native_sounds,
        "{name}: admitted native sound requests"
    );
    assert!(actor.mission_leaf.as_unit().is_none());
}

/// The current native Guard producer's ordinary stationary case, reached
/// through VERA's actual Infantry AI and mission dispatcher. Before the
/// producer is connected, Guard runs its Foot idle continuation instead.
#[test]
fn guard_auto_deploy_reaches_the_native_action_from_infantry_ai() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_deploy_action.json",
    ))
    .unwrap();
    let row = corpus
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["input"]["kind"] == "guard"
                && row["input"]["moving"] == 0
                && row["input"]["pending"] == 0
        })
        .unwrap();
    let (mut sim, rules, id) = native_deploy_fixture(row);
    sim.object_ai_visit_one(id, Some(&rules), Default::default());
    let actor = sim.substrate.entities.get(id).unwrap();
    assert_eq!(
        actor.mission_leaf.as_infantry().unwrap().doing(),
        row["after"]["doing"].as_i64().unwrap() as i32,
        "Guard must reach the original automatic Deploy action",
    );
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::EntityDeployed { .. }]
    ));
    assert!(actor.mission_leaf.as_unit().is_none());
}

#[test]
fn walk_stop_and_pending_callback_match_original_deployment_rows() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_deploy_action.json",
    ))
    .unwrap();
    let mut compared = 0;
    for row in corpus
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| matches!(row["input"]["kind"].as_str(), Some("stop" | "callback")))
    {
        let (mut sim, rules, id) = native_deploy_fixture(row);
        let untouched = (sim.main_rng.logical_state(), sim.mapgen_rng.logical_state());
        if row["input"]["kind"] == "stop" {
            sim.walk_stop_moving(id, Some(&rules), crate::sim::world::FrameEffects::default())
                .unwrap();
        } else {
            sim.infantry_pending_deploy_stop_callback(
                id,
                Some(&rules),
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        }
        assert_native_deploy_state(&sim, id, row);
        assert_eq!(
            (sim.main_rng.logical_state(), sim.mapgen_rng.logical_state()),
            untouched
        );
        compared += 1;
    }
    assert_eq!(compared, 196);
}

#[test]
fn deploy_completion_keeps_suffix_effects_when_next_action_refuses() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_deploy_action.json",
    ))
    .unwrap();
    let mut compared = 0;
    for row in corpus
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["kind"] == "completion")
    {
        let (mut sim, rules, id) = native_deploy_fixture(row);
        assert!(
            !sim.infantry_sequencer(id, &rules, crate::sim::world::FrameEffects::default()),
            "deployment does not UnInit"
        );
        assert_native_deploy_state(&sim, id, row);
        compared += 1;
    }
    assert_eq!(compared, 64);
}

#[test]
fn passive_scan_shortening_matches_original_signed_timer_and_rng_controls() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_deploy_action.json",
    ))
    .unwrap();
    let mut compared = 0;
    for row in corpus
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["kind"] == "reload")
    {
        let (mut sim, _rules, id) = native_deploy_fixture(row);
        // Historical packet label "map_editor" supplies A8E7AC, the shared
        // nesting counter. Exercise its real owner rather than a second
        // Boolean decision passed to the timer leaf.
        if row["input"]["map_editor"].as_u64().unwrap() != 0 {
            sim.with_object_placement_scope(|sim| sim.shorten_passive_scan_timer(id));
        } else {
            sim.shorten_passive_scan_timer(id);
        }
        assert_native_deploy_state(&sim, id, row);
        compared += 1;
    }
    assert_eq!(compared, 14);
}

#[test]
fn infantry_unload_full_original_handler_matches52_supplied_controls() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_mission_unload.json",
    ))
    .unwrap();
    for row in corpus.as_array().unwrap() {
        let (mut sim, rules, id) = native_deploy_fixture(row);
        assert_eq!(
            sim.main_rng.native_state_hex(),
            row["rng_streams_before"]["main"].as_str().unwrap()
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            row["rng_streams_before"]["mapgen"].as_str().unwrap()
        );
        let result = sim
            .infantry_mission_unload(id, &rules, crate::sim::world::FrameEffects::default())
            .unwrap();
        assert_eq!(
            result as u32,
            row["return_eax"].as_u64().unwrap() as u32,
            "{}: native return",
            row["input"]
        );
        assert_native_deploy_state(&sim, id, row);
    }
    assert_eq!(corpus.as_array().unwrap().len(), 52);
}

#[test]
fn synchronized_deploy_queues_unload_before_handler_action_and_sound() {
    use crate::sim::mission::{MissionId, MissionType};

    let row: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_mission_unload.json",
    ))
    .unwrap();
    let (mut sim, rules, id) = native_deploy_fixture(&row[0]);
    let actor = sim.substrate.entities.get(id).unwrap();
    let before_mission = actor.mission;
    let before_stage = serde_json::to_value(actor.native_stage()).unwrap();
    assert_eq!(before_mission.current().known(), Some(MissionType::Enter));
    assert!(sim.apply_command(
        "Americans",
        &Command::ToggleInfantryDeploy { entity_id: id },
        Some(&rules),
    ));
    let actor = sim.substrate.entities.get(id).unwrap();
    // Event4C73B4 pushes commenceNow0 before Queue4C73B9. This fixture
    // starts on Enter; publication queues Unload without promoting it.
    assert_eq!(actor.mission.current(), before_mission.current());
    assert_eq!(actor.mission.queued().known(), Some(MissionType::Unload));
    assert_eq!(actor.mission.effective().known(), Some(MissionType::Enter));
    assert_eq!(
        actor.mission.handler_state(),
        before_mission.handler_state()
    );
    assert_eq!(
        actor.mission.mission_start_frame(),
        before_mission.mission_start_frame()
    );
    assert_eq!(
        actor.mission.dispatch_timer(),
        before_mission.dispatch_timer()
    );
    assert_eq!(actor.mission_leaf.as_infantry().unwrap().doing(), 0);
    assert_eq!(
        serde_json::to_value(actor.native_stage()).unwrap(),
        before_stage
    );
    assert!(actor.navigation.nav_com.is_none());
    let locomotor = actor.locomotor.as_ref().unwrap();
    assert!(locomotor.walk_destination().is_none());
    assert_eq!(locomotor.walk_is_moving(), Some(false));
    assert!(actor.mission_leaf.as_unit().is_none());
    assert!(sim.sound_events.is_empty());
    // InfantryAI51BF03's Ready/Commence position promotes the stopped
    // actor's queue before dispatch may visit Mission_Unload51F6E0.
    let now = sim.session.binary_frame;
    sim.mission_host_promote(id, now, &rules);
    let actor = sim.substrate.entities.get(id).unwrap();
    assert_eq!(actor.mission.current().known(), Some(MissionType::Unload));
    assert_eq!(actor.mission.queued(), MissionId::NONE);
    assert_eq!(actor.mission_leaf.as_infantry().unwrap().doing(), 0);
    assert!(sim.sound_events.is_empty());
    // The supplied native-body fixture is directly inserted. Register it
    // through the existing Logic owner before exercising the master frame.
    assert!(sim.register_live_object(id));
    // Visit the production AI dispatcher after command publication/promotion.
    // The native-compared handler must be connected here, not only callable
    // by its isolated corpus test.
    sim.advance_tick(&[], Some(&rules), None, None, 66);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .mission_leaf
            .as_infantry()
            .unwrap()
            .doing(),
        27
    );
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::EntityDeployed { .. }]
    ));
}
