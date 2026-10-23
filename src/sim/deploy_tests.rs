//! Native deployment/Stop receivers and separate Unit/MCV regressions.

#![cfg(test)]

use crate::map::bridge_facts::{BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_STRUCTURAL};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::base_plan::{BasePlanNode, pack_base_plan_cell};
use crate::sim::combat::{Health, TargetKind};
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseAiActivationLatches;
use crate::sim::world::{SimSoundEvent, Simulation};

#[path = "ground_transport_deploy_tests.rs"]
mod ground_transport_deploy_tests;

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
    let id = sim.next_stable_object_id();
    sim.set_next_stable_object_id_for_test(sim.next_stable_object_id() + 1);
    let mut e = GameEntity::new_at_frame_zero_for_test(
        id,
        rx,
        ry,
        0,
        0,
        owner_id,
        Health::new(125),
        type_id,
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    // A directly-inserted GameEntity keeps the constructed `in_limbo` byte;
    // production spawns clear it through Reveal, and order admission reads it.
    e.lifecycle.set_in_limbo_for_test(false);
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
    sim.set_playfield_bounds_for_test(Some(crate::sim::arena_fixture::OPEN_PLAYFIELD));
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
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    add_house(&mut sim, "Americans", true);

    let start = (20u16, 22u16);
    let mcv = sim
        .spawn_object("AMCV", "Americans", start.0, start.1, 128, &rules)
        .expect("spawn MCV");
    sim.substrate
        .entities
        .get_mut(mcv)
        .unwrap()
        .assign_object_group(3);
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
        .find(|e| e.type_ref() == gacnst_id)
        .map(|e| e.stable_id())
        .expect("ConYard should exist after deploy");

    assert_eq!(sim.entities().get(yard).unwrap().object_group(), 3);

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
        .find(|e| e.type_ref() == amcv_id)
        .map(|e| {
            assert_eq!(e.object_group(), 3);
            (e.position.rx(), e.position.ry())
        })
        .expect("MCV should exist again after undeploy");
    assert_eq!(
        landed, start,
        "deploy/undeploy must round-trip; drifting here compounds every cycle"
    );
}

/// A computer house's MCV deploys into a yard that builds up before it opens:
/// Construction is queued before its Unlimbo (`0x007396D5`), so neither that
/// Unlimbo's owner discovery nor the deploy's own (`0x00739848`) opens it,
/// and the ready byte (`0x0073984E`) commences Construction on its first
/// Update. From the deploy frame on, each of its Updates matches the native
/// deploy `route` rows (`tools/spatial_oracle/building_construction.json`),
/// up to the opening.
#[test]
fn a_computer_mcv_deploys_into_a_yard_that_follows_the_native_deploy_route() {
    use crate::sim::building_construction::native_route;
    use crate::sim::mission::{MissionId, MissionType};
    let rows = native_route::rows("deploy");
    assert_eq!(rows.len(), 6);
    for row in &rows {
        let name = row["input"]["name"].as_str().unwrap();
        let mut rules = make_recalc_mcv_rules("0,0,0");
        rules.set_buildup_control_for_test("GACNST", native_route::control(row));
        let mut sim = deploy_sim(&rules);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        // The native rows run at GameSpeed 0.
        sim.session.game_options_mut_for_test().game_speed = 0;
        add_house(&mut sim, "Americans", false);
        let mcv = sim
            .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
            .expect("spawn MCV");
        let deployed_at = sim.session.binary_frame();
        assert!(sim.deploy_mcv(
            mcv,
            &rules,
            None,
            crate::sim::world::FrameEffects::default()
        ));
        let yard = deployed_type(&sim, "GACNST");
        let id = yard.stable_id();
        assert!(
            !yard.building_art.actually_placed(),
            "{name}: opened at deploy"
        );
        assert_eq!(yard.mission.current(), MissionId::NONE, "{name}");
        assert_eq!(
            yard.mission.queued(),
            MissionId::from_known(MissionType::Construction),
            "{name}"
        );
        assert_eq!(yard.building_ready_latch(), 1, "{name}");
        for frame in row["frames"].as_array().unwrap() {
            let now = deployed_at + frame["frame"].as_u64().unwrap() as u32;
            sim.session.set_binary_frame_for_test(now);
            sim.object_ai_visit_one(id, Some(&rules), crate::sim::world::ObjectAiCtx::default());
            let yard = sim.substrate.entities.get(id).unwrap();
            if native_route::assert_frame(yard, frame, &format!("{name} frame {now}")) {
                break;
            }
        }
    }
}

#[test]
fn deploy_mcv_accepts_mixed_height_clear_foundation() {
    let rules = make_mcv_rules();
    let mut sim = deploy_sim(&rules);
    sim.resolved_terrain
        .as_mut()
        .expect("deploy fixture terrain")
        .cell_mut_for_test(20, 21)
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
        sim.substrate.entities.values().any(|e| {
            e.type_ref() == gacnst_id && e.position.rx() == 19 && e.position.ry() == 21
        }),
        "Construction Yard should spawn at gamemd's deploy foundation origin"
    );
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
fn deploy_mcv_rejects_pure_0x400_bridge_marker_foundation_cell() {
    let terrain = mcv_deploy_terrain_with(|cell| {
        cell.bridge_facts.raw_flags = BRIDGE_FLAG_DESTROYED_OR_RAMP;
    });

    let (applied, mcv_remains, sound_count) = deploy_mcv_with_terrain(terrain);

    assert!(!applied);
    assert!(mcv_remains);
    assert_eq!(sound_count, 1);
}

/// `UnitClass::Deploy` reaches Computer_Paranoid through `0x00505180`
/// (`0x007398D9`, `0x005051AF`), after the base center and before the
/// Recalc: a computer MCV's deploy allies the computer houses against the
/// human.
#[test]
fn a_computer_mcv_deploy_turns_the_computers_paranoid() {
    let mut rules = make_recalc_mcv_rules("0,0,0");
    rules.general.allies_allowed = true;
    let mut sim = deploy_sim(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    // Every house is of the fixture's one country, which `[Sides]` lists as
    // Allied: a type outside every list would answer Find_Index's -1 for a
    // ruleset without a `Civilian` side, and Can_Ally refuses that house.
    let americans = sim.interner.intern("Americans");
    for (owner, human) in [("Player", true), ("Americans", false), ("Computer2", false)] {
        let id = sim.interner.intern(owner);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                id,
                0,
                Some(americans),
                human,
                5000,
                10,
            ));
    }
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
        .expect("spawn MCV");
    assert!(sim.house_alliances.as_map().is_empty());
    assert!(sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    let allies = |name: &str| sim.house_alliances.as_map()[&name.to_ascii_uppercase()].clone();
    assert!(allies("Americans").contains("COMPUTER2"));
    assert!(allies("Computer2").contains("AMERICANS"));
    let player = sim.interner.get("Player").unwrap();
    for computer in ["Americans", "Computer2"] {
        let id = sim.interner.get(computer).unwrap();
        assert_eq!(sim.houses[&id].enemy_house(), Some(player), "{computer}");
    }
}

fn add_house(sim: &mut Simulation, owner: &str, is_human: bool) {
    let owner_id = sim.interner.intern(owner);
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner_id,
            0,
            Some(owner_id),
            is_human,
            5000,
            10,
        ));
}

fn deployed_type<'a>(sim: &'a Simulation, type_name: &str) -> &'a GameEntity {
    let type_id = sim.interner.get(type_name).expect("deployed type interned");
    sim.substrate
        .entities
        .values()
        .find(|entity| entity.type_ref() == type_id && !entity.death.is_dying())
        .expect("deployed target exists")
}

#[test]
fn base_plan_recalc_deploy_generates_and_anchors_nonhuman_conyard() {
    let rules = make_recalc_mcv_rules("0,0,0");
    let mut sim = deploy_sim(&rules);
    add_house(&mut sim, "Americans", false);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
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
    assert_eq!((yard.position.rx(), yard.position.ry()), (19, 21));
    assert!(yard.building_up());
    assert!(
        yard.ai_sell_repair.ai_repairable(),
        "a computer's yard is AI-repairable"
    );
    let owner = sim.interner.get("Americans").unwrap();
    let house = &sim.houses[&owner];
    assert_eq!(house.base_center(), Some((19, 21)));
    assert_eq!(house.base_plan_center(), (19, 21));
    assert_eq!(house.ai_activation(), ENABLED_AI_ACTIVATION);
    assert!(!house.base_plan().nodes().is_empty());
    assert_eq!(
        house.base_plan().nodes()[0].packed_cell,
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
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    add_house(&mut sim, "Americans", false);
    let owner = sim.interner.get("Americans").unwrap();
    let game_mode_nonzero = sim.session.game_mode_nonzero();
    let house = sim.houses.get_mut(&owner).unwrap();
    house.set_current_iq_for_test(5);
    // Scenario setup's SetDifficulty supplies the RepairDelay the latch timer
    // draws from; the House update releases the latch once that expires.
    house.set_difficulty(
        crate::sim::house_state::HouseDifficulty::Normal,
        &rules.general,
        Default::default(),
        game_mode_nonzero,
        0,
        0,
    );
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
        .expect("spawn MCV");
    sim.substrate
        .entities
        .get_mut(mcv)
        .unwrap()
        .health
        .set_for_test(225);
    let deployed_at = sim.session.binary_frame();
    assert!(sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    sim.flush_pending_delete();
    let yard = deployed_type(&sim, "GACNST").stable_id();
    let mut frames = Vec::new();
    for _ in 0..7 {
        let frame = sim.session.binary_frame() - deployed_at;
        tick_n(&mut sim, &rules, 1);
        let entity = sim.substrate.entities.get(yard).unwrap();
        let house = &sim.houses[&owner];
        frames.push((
            frame,
            entity.building_up(),
            entity.repairing.is_on(),
            house.repair_start_latch(),
        ));
    }
    // A deploy's 3x2 build-up completes at D + 1 + (3 - 1) * 2 (the
    // `building_construction.json` `route` rows).
    let expected: Vec<_> = (0..7)
        .map(|frame| (frame, frame < 5, frame >= 5, frame >= 5))
        .collect();
    assert_eq!(frames, expected);
    assert_eq!(
        sim.houses[&owner].repair_latch_timer().start_frame(),
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
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_ai_activation_for_test(SPLIT_AI_ACTIVATION);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(game_mode_nonzero),
        );
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
        assert_eq!(
            deployed_type(&sim, "GACNST").ai_sell_repair.ai_repairable(),
            !is_human
        );
        let house = &sim.houses[&owner];
        assert_eq!(house.base_center(), None);
        assert_eq!(house.base_plan_center(), (0, 0));
        assert!(house.base_plan().nodes().is_empty());
        assert_eq!(house.ai_activation(), SPLIT_AI_ACTIVATION);
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
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_ai_activation_for_test(SPLIT_AI_ACTIVATION);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
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
    assert!(deployed_type(&sim, "YAREFN").ai_sell_repair.ai_repairable());
    assert_eq!(sim.houses[&owner].base_center(), None);
    assert_eq!(sim.houses[&owner].base_plan_center(), (0, 0));
    assert!(sim.houses[&owner].base_plan().nodes().is_empty());
    assert_eq!(sim.houses[&owner].ai_activation(), SPLIT_AI_ACTIVATION);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(miner).is_none());
}

/// A human House keeps no base cell through its deploy: Building Unlimbo
/// recalculates (`0x00440D07` -> `0x004FD150`, which stores the empty cell
/// first) before the yard joins House+68 (`0x00441553..0x00441594`), and
/// UnitClass::Deploy skips Set_Starting_Cell for a House a human controls
/// (`0x0073985B..0x00739862`). The House's next Building Unlimbo counts the
/// yard but not itself. The computer's Spy Plane, Paradrop and zone-tested
/// teams read this cell (`0x00509CD0`, `0x0041FEE0`).
#[test]
fn a_human_deploy_empties_the_base_cell_until_the_next_building_counts_the_yard() {
    let rules = make_recalc_mcv_rules("0,0,0");
    let mut sim = deploy_sim(&rules);
    add_house(&mut sim, "Americans", true);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let owner = sim.interner.get("Americans").unwrap();
    // The launch's start cell; the MCV has since driven to (20,22).
    sim.houses.get_mut(&owner).unwrap().set_base_center((6, 6));
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
        .expect("spawn MCV");

    assert!(sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(deployed_type(&sim, "GACNST").building_up());
    assert_eq!(sim.houses[&owner].base_center(), None);
    assert_eq!(sim.houses[&owner].base_origin(), (0, 0));

    // Far from the yard (4x3 at (19,21)): were it counted, the centre would
    // move halfway towards it.
    sim.spawn_object("GAPOWR", "Americans", 28, 8, 0, &rules)
        .expect("spawn power plant");
    let (x, y) = sim.houses[&owner]
        .base_center()
        .expect("the yard gives a base cell");
    assert!(
        (18..=23).contains(&x) && (20..=24).contains(&y),
        "base cell {:?} is not at the yard",
        (x, y)
    );
}

#[test]
fn base_plan_recalc_deploy_countryless_nonempty_plan_only_reanchors_node_zero() {
    let rules = make_recalc_mcv_rules("");
    let mut sim = deploy_sim(&rules);
    add_house(&mut sim, "Americans", false);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.scenario_rng = crate::sim::rng::SimRng::new(0x1357_2468);
    let owner = sim.interner.get("Americans").unwrap();
    let house = sim.houses.get_mut(&owner).unwrap();
    house.set_country_for_test(None);
    house.set_ai_activation_for_test(SPLIT_AI_ACTIVATION);
    house.base_plan_mut().set_nodes_for_test(vec![BasePlanNode {
        type_or_control: 4,
        packed_cell: 0xAABB_CCDD,
        filled: true,
        retry_count: -7,
    }]);
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
    assert_eq!(house.base_center(), Some((19, 21)));
    assert_eq!(house.base_plan_center(), (19, 21));
    assert_eq!(house.ai_activation(), ENABLED_AI_ACTIVATION);
    assert_eq!(house.base_plan().nodes().len(), 1);
    assert_eq!(house.base_plan().nodes()[0].type_or_control, 4);
    assert_eq!(
        house.base_plan().nodes()[0].packed_cell,
        pack_base_plan_cell(19, 21)
    );
    assert!(house.base_plan().nodes()[0].filled);
    assert_eq!(house.base_plan().nodes()[0].retry_count, -7);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(mcv).is_none());
}

#[test]
fn base_plan_recalc_deploy_failures_preserve_source_rng_plan_and_centers() {
    let valid_rules = make_recalc_mcv_rules("0,0,0");
    let malformed_rules = make_recalc_mcv_rules("");

    for (rules, add_blocker) in [(&valid_rules, true), (&malformed_rules, false)] {
        let mut sim = deploy_sim(rules);
        add_house(&mut sim, "Americans", false);
        let owner = sim.interner.get("Americans").unwrap();
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_ai_activation_for_test(SPLIT_AI_ACTIVATION);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.scenario_rng = crate::sim::rng::SimRng::new(0xDEAD_BEEF);
        let mcv = sim
            .spawn_object("AMCV", "Americans", 20, 22, 128, rules)
            .expect("spawn MCV");
        if add_blocker {
            add_house(&mut sim, "Blocker", false);
            sim.spawn_object("GAPOWR", "Blocker", 21, 22, 0, rules)
                .expect("spawn footprint blocker");
        }
        let rng_before = sim.scenario_rng.state();

        assert!(!sim.deploy_mcv(mcv, rules, None, crate::sim::world::FrameEffects::default()));
        assert!(!sim.substrate.entities.get(mcv).unwrap().death.is_dying());
        let house = &sim.houses[&owner];
        assert_eq!(house.base_center(), None);
        assert_eq!(house.base_plan_center(), (0, 0));
        assert!(house.base_plan().nodes().is_empty());
        assert_eq!(house.ai_activation(), SPLIT_AI_ACTIVATION);
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
    sim.session.game_options_mut_for_test().mcv_redeploy = false;

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
    sim.session.game_options_mut_for_test().mcv_redeploy = false;

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
         [Guard]\nRate=0.1\n[Sticky]\nRate=0.1\n[Area Guard]\nRate=0.1\n[Move]\nRate=0.1\n\
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
    position.set_rx_for_test((x / 256).clamp(0, i32::from(u16::MAX)) as u16);
    position.set_ry_for_test((y / 256).clamp(0, i32::from(u16::MAX)) as u16);
    position.set_sub_x_for_test(SimFixed::from_num(x - i32::from(position.rx()) * 256));
    position.set_sub_y_for_test(SimFixed::from_num(y - i32::from(position.ry()) * 256));
    position.set_exact_z_leptons_for_test(Some(xyz[2].as_i64().unwrap() as i32));
}

pub(crate) fn native_deploy_fixture(row: &serde_json::Value) -> (Simulation, RuleSet, u64) {
    use crate::sim::mission::MissionId;
    use crate::util::fixed_math::SimFixed;
    let input = &row["input"];
    let before = &row["before"];
    let rules = native_deploy_rules(input);
    let mut sim = Simulation::with_seed(31);
    sim.mapgen_rng = crate::sim::rng::SimRng::new(31);
    sim.session
        .set_binary_frame_for_test(input["now"].as_i64().unwrap_or(100) as u32);
    sim.session.game_options_mut_for_test().game_speed =
        input["game_speed_index"].as_i64().unwrap_or(0) as i32;
    sim.session.set_game_mode(
        crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
            input["game_mode_nonzero"].as_bool().unwrap_or(false),
        ),
    );
    let owner = sim.interner.intern("Americans");
    let human = input["human"].as_i64().unwrap_or(0) != 0;
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, human, 0, 10,
        ));
    sim.houses.get_mut(&owner).unwrap().set_difficulty(
        crate::sim::house_state::HouseDifficulty::from_native(
            input["difficulty"].as_i64().unwrap_or(1) as i32,
        )
        .unwrap(),
        &rules.general,
        Default::default(),
        sim.session.game_mode_nonzero(),
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
    sim.substrate
        .mission_assign_exact(
            id,
            MissionId::from_raw(input["mission"].as_i64().unwrap_or(5) as i32),
            input["mission_start"].as_i64().unwrap_or(0) as u32,
        )
        .unwrap();
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor
        .health
        .set_for_test(input["health"].as_i64().unwrap_or(100) as i32);
    actor.position.set_sub_x_for_test(SimFixed::from_num(128));
    actor.position.set_sub_y_for_test(SimFixed::from_num(128));
    actor.position.set_exact_z_leptons_for_test(Some(0));
    if input["position"].is_array() {
        supplied_world_position(&mut actor.position, &input["position"]);
    }
    actor
        .infantry
        .as_mut()
        .unwrap()
        .set_is_prone_for_test(before["prone"] != 0);
    if before["idle_timer"].is_array() {
        actor
            .infantry
            .as_mut()
            .unwrap()
            .set_idle_action_timer_for_test(crate::sim::mission::MissionTimer::armed(
                before["idle_timer"][0].as_i64().unwrap() as u32,
                before["idle_timer"][1].as_i64().unwrap() as u32,
            ));
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
    actor
        .passive_scan
        .set_passive_scan_timer_for_test(crate::sim::mission::MissionTimer::armed(
            before["reload"][0].as_i64().unwrap() as u32,
            before["reload"][1].as_i64().unwrap() as u32,
        ));
    let mut loco = crate::sim::movement::locomotor::LocomotorState::from_object_type(
        rules
            .object(input["type_id"].as_str().unwrap_or("E1"))
            .unwrap(),
        0,
    );
    // Supplied raw prior bytes are independent: a retained destination need
    // not have moving34 set, and motion36 can survive a paid-head release.
    // MoveTo would invent a transition instead of transporting the fixture.
    loco.set_runtime_payload_for_test(
        crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload::Walk(
            crate::sim::movement::locomotion::piggyback::WalkRuntime::for_test(
                native_coord(&before["head"]),
                native_coord(&before["destination"]),
                before["moving"] != 0,
                before["motion"] != 0,
            ),
        ),
    );
    actor.locomotor = Some(loco);
    if before["nav"].as_i64().unwrap_or(0) != 0 {
        actor
            .navigation
            .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::cell(11, 10)));
    }
    if before["target"].as_i64().unwrap_or(0) != 0 {
        actor
            .attack_target
            .set_for_test(Some(TargetKind::Cell(10, 10)));
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
        actor.infantry.as_ref().unwrap().is_prone(),
        expected["prone"] != 0,
        "{name}: prone6DB"
    );
    let stage = actor.native_stage();
    if expected["idle_timer"].is_array() {
        let timer = actor.infantry.as_ref().unwrap().idle_action_timer();
        assert_eq!(
            serde_json::json!([timer.start_frame() as i32, timer.duration() as i32]),
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
            actor.passive_scan.timer().start_frame() as i32,
            actor.passive_scan.timer().duration() as i32
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
            actor.navigation.nav_com().is_some(),
            expected["nav"] != 0,
            "{name}: NavCom"
        );
        assert_eq!(
            actor.archive_target().is_some(),
            expected["archive"] != 0,
            "{name}: archive identity retained"
        );
        let target = actor.attack_target.target();
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
        .filter(|&event| event["address"] == "007509E0")
        .map(|event| event["sound"].as_u64().unwrap())
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
            sim.walk_stop_moving(id, &rules, crate::sim::world::FrameEffects::default())
                .unwrap();
        } else {
            sim.infantry_pending_deploy_stop_callback(
                id,
                &rules,
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
    assert!(actor.navigation.nav_com().is_none());
    let locomotor = actor.locomotor.as_ref().unwrap();
    assert!(locomotor.walk_destination().is_none());
    assert_eq!(locomotor.walk_is_moving(), Some(false));
    assert!(actor.mission_leaf.as_unit().is_none());
    assert!(sim.sound_events.is_empty());
    // InfantryAI51BF03's Ready/Commence position promotes the stopped
    // actor's queue before dispatch may visit Mission_Unload51F6E0.
    let now = sim.session.binary_frame();
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

/// CanDeploySlashUnload (`0x00700D50`) for the transport fixture at
/// (20, 22): `mutate` edits each terrain cell before the query.
fn transport_may_unload(
    human: bool,
    building: Option<&str>,
    mutate: impl Fn(&mut ResolvedTerrainCell, (u16, u16)),
    prepare: impl FnOnce(&mut GameEntity),
) -> bool {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=LCRF\n[BuildingTypes]\n0=SHED\n1=BUNK\n\
         [LCRF]\nPassengers=5\n[SHED]\nStrength=100\n[BUNK]\nStrength=100\nBunker=yes\n",
    ))
    .expect("transport rules");
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, human, 0, 10,
        ));
    if let Some(building) = building {
        sim.spawn_object(building, "Americans", 20, 22, 0, &rules)
            .expect("the building builds");
    }
    let id = sim
        .spawn_object("LCRF", "Americans", 20, 22, 0, &rules)
        .expect("the transport builds");
    prepare(sim.substrate.entities.get_mut(id).unwrap());
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(32, 32, |rx, ry| {
        let mut cell = crate::map::resolved_terrain::test_tiberium_cell(rx, ry);
        mutate(&mut cell, (rx, ry));
        cell
    }));
    sim.can_deploy_slash_unload(id, &rules)
}

/// The transport arm (`Passengers=` above 0, `0x00700EEC..0x007010BB`):
/// refused on a bridge, beside a bridge deck (own cell or a N/S/E/W
/// neighbour, not a diagonal), on Water when its owner is human, and in a
/// building's cell unless the building is a `Bunker=`.
#[test]
fn a_transport_may_not_unload_on_or_beside_a_bridge_on_human_water_or_in_a_building() {
    let deck_at = |at: (u16, u16)| {
        move |cell: &mut ResolvedTerrainCell, here: (u16, u16)| {
            if here == at {
                cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
            }
        }
    };
    let water = |cell: &mut ResolvedTerrainCell, here: (u16, u16)| {
        if here == (20, 22) {
            cell.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
        }
    };
    let plain = |_: &mut ResolvedTerrainCell, _: (u16, u16)| {};
    assert!(transport_may_unload(true, None, plain, |_| {}));
    assert!(!transport_may_unload(true, None, plain, |e| e.set_on_bridge(true)));
    for at in [(20, 22), (20, 21), (20, 23), (19, 22), (21, 22)] {
        assert!(
            !transport_may_unload(true, None, deck_at(at), |_| {}),
            "{at:?}"
        );
    }
    assert!(transport_may_unload(true, None, deck_at((21, 21)), |_| {}));
    assert!(!transport_may_unload(true, None, water, |_| {}));
    assert!(transport_may_unload(false, None, water, |_| {}));
    assert!(!transport_may_unload(true, Some("SHED"), plain, |_| {}));
    assert!(transport_may_unload(true, Some("BUNK"), plain, |_| {}));
}

/// The Unit arm's capability and DeploysInto tests, the Infantry
/// `UndeployDelay=` and `Deployer=` tests, and the Building and Aircraft
/// arms of CanDeploySlashUnload (`0x00700D50`).
#[test]
fn can_deploy_slash_unload_follows_each_class_arm() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MCV\n1=TANK\n2=FIRER\n3=REFINER\n\
         [InfantryTypes]\n0=YURI\n1=GI\n2=E2\n\
         [AircraftTypes]\n0=JET\n1=CARRIER\n\
         [BuildingTypes]\n0=YARD\n1=REFINERY\n\
         [MCV]\nDeploysInto=YARD\n[TANK]\nStrength=100\n[FIRER]\nDeployFire=yes\n\
         [YURI]\nDeployer=yes\nUndeployDelay=150\n[GI]\nDeployer=yes\n[E2]\nStrength=100\n\
         [JET]\nStrength=100\n[CARRIER]\nPassengers=2\n\
         [YARD]\nConstructionYard=yes\nUndeploysInto=MCV\n\
         [REFINER]\nDeploysInto=REFINERY\n[REFINERY]\nStrength=100\n",
    ))
    .expect("deploy rules");
    let mut sim = Simulation::new();
    let spawn = |sim: &mut Simulation, type_id: &str, rx: u16| {
        sim.spawn_object(type_id, "Americans", rx, 5, 0, &rules)
            .expect("the fixture builds")
    };
    let [mcv, tank, firer, yuri, gi, e2, jet, carrier, yard, refiner] = [
        "MCV", "TANK", "FIRER", "YURI", "GI", "E2", "JET", "CARRIER", "YARD", "REFINER",
    ]
    .map(|type_id| {
        let rx = 2 + 3 * sim.substrate.entities.len() as u16;
        spawn(&mut sim, type_id, rx)
    });
    let can = |sim: &Simulation, id| sim.can_deploy_slash_unload(id, &rules);
    assert_eq!(
        [mcv, tank, firer, yuri, gi, e2, jet, carrier, yard, refiner].map(|id| can(&sim, id)),
        [
            true, false, true, true, true, false, false, true, false, true
        ]
    );

    // A parasite bars any DeploysInto unit; mind control one whose
    // DeploysInto is a construction yard.
    fn entity(sim: &mut Simulation, id: u64) -> &mut GameEntity {
        sim.substrate.entities.get_mut(id).unwrap()
    }
    entity(&mut sim, mcv)
        .parasite
        .set_eater_for_test(Some(tank));
    assert!(!can(&sim, mcv));
    entity(&mut sim, mcv).parasite.set_eater_for_test(None);
    entity(&mut sim, mcv).mind_control =
        crate::sim::capture_manager::MindControlLink::controlled_by_for_test(tank);
    assert!(!can(&sim, mcv));
    // A mind-controlled unit deploying into anything else may.
    entity(&mut sim, refiner).mind_control =
        crate::sim::capture_manager::MindControlLink::controlled_by_for_test(tank);
    assert!(can(&sim, refiner));

    // `UndeployDelay=` refuses while deployed, in Doing 31, or rearming.
    let frame = sim.session.binary_frame() as i32;
    for doing in [28, 31] {
        for id in [yuri, gi] {
            entity(&mut sim, id)
                .mission_leaf
                .set_infantry_doing_verified(doing)
                .unwrap();
        }
        assert_eq!((can(&sim, yuri), can(&sim, gi)), (false, true), "{doing}");
    }
    for id in [yuri, gi] {
        let soldier = entity(&mut sim, id);
        soldier.mission_leaf.set_infantry_doing_verified(0).unwrap();
        soldier
            .rearm_timer
            .set_for_test(crate::sim::timer::CdTimer::started(frame, 40));
    }
    assert_eq!((can(&sim, yuri), can(&sim, gi)), (false, true));
}

/// The DEPLOY event admits a retail Chaos Drone (`[CAOS]`, `DeployFire=`)
/// and queues Unload. `UnitClass::Mission_Unload`'s deploy-fire arm
/// (`0x0073DCD3`) itself targets the drone's own cell, fires weapon 0
/// (ChaosAttack) there, keeps that Target and returns `RandomRanged(0, 2)`
/// plus 14; called directly, so the every-frame Fire_At_Target cannot stand
/// in for its shot.
#[test]
fn a_chaos_drone_deploy_fires_its_weapon_at_its_own_cell() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &battle.rules;
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    let drone = sim
        .spawn_object("CAOS", "YuriCountry", 10, 10, 64, rules)
        .expect("the retail Chaos Drone builds");
    assert!(sim.apply_command(
        "YuriCountry",
        &Command::DeployMcv { entity_id: drone },
        Some(rules)
    ));
    let entity = sim.substrate.entities.get(drone).unwrap();
    assert_eq!(
        entity.mission.queued().known(),
        Some(crate::sim::mission::MissionType::Unload)
    );
    assert_eq!(entity.rearm_timer.duration(), 0);
    assert_eq!(entity.attack_target.target(), None);

    let (delay, _) = sim.unit_deploy_fire_unload(
        drone,
        rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let entity = sim.substrate.entities.get(drone).unwrap();
    assert!((14..=16).contains(&delay), "{delay}");
    assert!(entity.rearm_timer.duration() > 0, "the arm fired");
    assert_eq!(
        entity.attack_target.target(),
        Some(TargetKind::Cell(10, 10))
    );
}
