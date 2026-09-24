//! Unit tests for the `Spawns=` pool (GSI-05.08 gap 1).
//!
//! Coverage is scoped to the mechanism this slice landed: pool construction,
//! the update-timer gate (negative half only), the `Spawner=yes` fire
//! hand-off, the missile launch path (stationary gate, kamikaze window, flight
//! speed), the impact damage, both `Kill_All_Spawns` entry points, and the
//! aircraft dock (`LandingAtDock` → `Reloading` → `ReadyDocked`).
//!
//! NOT covered here: the slot's `Regenerating` → `ReadyDocked` rebuild after
//! `SpawnRegenRate`, the `ReturningToDock` → `LandingAtDock` hand-off, and the
//! update timer's positive edge.

#![cfg(test)]

use std::collections::BTreeMap;

use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::rules::missile_spawn::MissileFamily;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
use crate::sim::combat::TargetKind;
use crate::sim::spawn_manager::{
    SpawnManagerMode, SpawnSlotState, SpawnTimer, tick_spawn_managers,
};
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

/// A V3 Launcher (missile pool of 1), a Dreadnought (missile pool of 2), an
/// Aircraft Carrier (aircraft pool of 3), and stationary/mobile targets.
///
/// Values mirror the stock `rulesmd.ini` sections named in the survey:
/// `[V3]` L7740, `[DRED]` L8125, `[CARRIER]` L7255.
fn make_spawner_rules() -> RuleSet {
    make_spawner_rules_with_hornet_strength(75)
}

fn make_spawner_rules_with_hornet_strength(strength: i32) -> RuleSet {
    let text = spawner_rules_text().replace(
        "[HORNET]\nName=Hornet\nStrength=75",
        &format!("[HORNET]\nName=Hornet\nStrength={strength}"),
    );
    RuleSet::from_ini(&IniFile::from_str(&text)).expect("spawner rules should parse")
}

fn spawner_rules_text() -> &'static str {
    "\
[General]
BuildSpeed=0.75
MultipleFactory=0.7
LowPowerPenaltyModifier=1.25
MinLowPowerProductionSpeed=0.4
MaxLowPowerProductionSpeed=0.85
V3RocketPauseFrames=0
V3RocketTiltFrames=60
V3RocketDamage=200
V3RocketEliteDamage=400
V3RocketType=V3ROCKET
DMislPauseFrames=20
DMislTiltFrames=60
DMislDamage=300
DMislType=DMISL
CMislType=CMISL

[CombatDamage]
V3Warhead=V3WH
V3EliteWarhead=V3EWH
DMislWarhead=DMISLWH
DMislEliteWarhead=DMISLEWH
CMislWarhead=CMISLWH
CMislEliteWarhead=CMISLEWH

[InfantryTypes]

[VehicleTypes]
0=V3
1=DRED
2=CARRIER
3=MOBILE

[AircraftTypes]
0=V3ROCKET
1=DMISL
2=HORNET

[BuildingTypes]
0=TARGET
1=FRAGILE

[Warheads]
0=Special
1=V3WH
2=V3EWH
3=DMISLWH
4=DMISLEWH
5=CMISLWH
6=CMISLEWH

[V3]
Name=V3 Launcher
Cost=800
Strength=150
Armor=light
Speed=4
Sight=20
Primary=V3Launcher
Spawns=V3ROCKET
SpawnsNumber=1
SpawnRegenRate=400
SpawnReloadRate=0
NoSpawnAlt=yes

[DRED]
Name=Dreadnought
Cost=2000
Strength=800
Armor=heavy
Speed=5
Sight=20
Primary=DredLauncher
Spawns=DMISL
SpawnsNumber=2
SpawnRegenRate=80
SpawnReloadRate=0

[CARRIER]
Name=Aircraft Carrier
Cost=2000
Strength=800
Armor=heavy
Speed=5
Sight=20
Primary=HornetLauncher
Spawns=HORNET
SpawnsNumber=3
SpawnRegenRate=600
SpawnReloadRate=150

[MOBILE]
Name=Mobile Target
Strength=500
Armor=heavy
Speed=5

[V3ROCKET]
Name=V3 Rocket
Strength=50
Armor=special_2
Speed=15
Locomotor={B7B49766-E576-11d3-9BD9-00104B972FE8}
MovementZone=Fly
Spawned=yes
MissileSpawn=yes
Ammo=1

[DMISL]
Name=Dread Missile
Strength=50
Armor=special_2
Speed=18
Locomotor={B7B49766-E576-11d3-9BD9-00104B972FE8}
MovementZone=Fly
Spawned=yes
MissileSpawn=yes
Ammo=1

[HORNET]
Name=Hornet
Strength=75
Armor=light
Speed=12
Locomotor={4A582746-9839-11d1-B709-00A024DDAFD1}
MovementZone=Fly
Spawned=yes
Primary=HornetBomb
Ammo=1

[TARGET]
Name=Target
Strength=1000
Armor=heavy
Foundation=1x1

[FRAGILE]
Name=Fragile Target
Strength=50
Armor=heavy
Foundation=1x1

[V3Launcher]
Damage=1
ROF=150
Range=18
MinimumRange=5
Spawner=yes
Projectile=InvisibleHigh
Speed=10
Warhead=Special

[DredLauncher]
Damage=50
ROF=50
Range=25
Spawner=yes
Projectile=InvisibleHigh
Speed=15
Warhead=Special

[HornetLauncher]
Damage=1
ROF=150
Range=25
Spawner=yes
Projectile=Invisible
Speed=10
Warhead=Special

[HornetBomb]
Damage=60
ROF=50
Range=4
Projectile=Invisible
Speed=20
Warhead=Special

[Special]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%

[V3WH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
CellSpread=1.5

[V3EWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
CellSpread=1.5

[DMISLWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
CellSpread=1.5

[DMISLEWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
CellSpread=1.5

[CMISLWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%

[CMISLEWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
"
}

fn empty_height_map() -> BTreeMap<(u16, u16), u8> {
    BTreeMap::new()
}

fn flat_terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    let speed_costs = SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };
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
        height_in_pixels: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class: TerrainClass::Clear,
        speed_costs,
        is_water: false,
        is_cliff_like: false,
        is_rough: false,
        is_road: false,
        accepts_smudge: true,
        allows_tiberium: true,
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
        base_terrain_class: TerrainClass::Clear,
        base_speed_costs: speed_costs,
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

fn flat_sim() -> Simulation {
    const WIDTH: u16 = 40;
    const HEIGHT: u16 = 32;
    let cells = (0..HEIGHT)
        .flat_map(|ry| (0..WIDTH).map(move |rx| flat_terrain_cell(rx, ry)))
        .collect();
    let mut sim = Simulation::new();
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -40,
        off_100: -1,
        off_104: 80,
        off_108: 41,
    });
    sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(WIDTH, HEIGHT, cells));
    sim
}

fn move_target_to_x_distance(sim: &mut Simulation, target_id: u64, distance_leptons: i32) {
    const OWNER_WORLD_X: i32 = 10 * 256 + 128;
    let world_x = OWNER_WORLD_X + distance_leptons;
    let target = sim
        .substrate
        .entities
        .get_mut(target_id)
        .expect("target remains live before manager update");
    target.position.rx = world_x.div_euclid(256) as u16;
    target.position.ry = 10;
    target.position.sub_x = SimFixed::from_num(world_x.rem_euclid(256));
    target.position.sub_y = SimFixed::from_num(128);
}

#[test]
fn v3_launcher_gets_a_missile_pool_on_spawn() {
    let rules = make_spawner_rules();
    let mut sim = Simulation::new();
    let hm = empty_height_map();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");

    let manager = sim
        .substrate
        .entities
        .get(v3)
        .and_then(|e| e.spawn_manager.as_ref())
        .expect("V3 should carry a spawn manager");
    assert_eq!(manager.slots.len(), 1, "SpawnsNumber=1");
    assert_eq!(manager.regen_rate, 400);
    assert_eq!(manager.missile_family, Some(MissileFamily::V3Rocket));
    // Native creates the children in the constructor, so the pool is already
    // full before the first fire attempt.
    assert_eq!(manager.count_alive_spawns(), 1);
    assert_eq!(manager.slots[0].state, SpawnSlotState::ReadyDocked);
    assert!(manager.slots[0].is_missile_spawn);

    let child_id = manager.slots[0].spawn.expect("child materialised");
    let child = sim.substrate.entities.get(child_id).expect("child exists");
    assert!(
        child.lifecycle.in_limbo,
        "a docked spawn child sits in limbo until launch"
    );
    assert_eq!(child.spawn_owner_id, Some(v3), "back-pointer to the parent");
}

#[test]
fn carrier_pool_is_not_missile_flavoured() {
    let rules = make_spawner_rules();
    let mut sim = Simulation::new();
    let hm = empty_height_map();
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules, &hm)
        .expect("spawn CARRIER");
    let manager = sim
        .substrate
        .entities
        .get(carrier)
        .and_then(|e| e.spawn_manager.as_ref())
        .expect("carrier manager");
    assert_eq!(manager.slots.len(), 3);
    assert_eq!(manager.reload_rate, 150);
    assert_eq!(
        manager.missile_family, None,
        "HORNET is not one of the three hardcoded rocket families"
    );
    assert!(manager.slots.iter().all(|s| !s.is_missile_spawn));
}

#[test]
fn units_without_spawns_get_no_manager() {
    let rules = make_spawner_rules();
    let mut sim = Simulation::new();
    let hm = empty_height_map();
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules, &hm)
        .expect("spawn TARGET");
    assert!(
        sim.substrate
            .entities
            .get(target)
            .expect("target")
            .spawn_manager
            .is_none()
    );
}

#[test]
fn set_target_queues_and_the_ai_pass_promotes_it() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let hm = empty_height_map();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules, &hm)
        .expect("spawn TARGET");

    if let Some(manager) = sim
        .substrate
        .entities
        .get_mut(v3)
        .and_then(|e| e.spawn_manager.as_mut())
    {
        manager.set_target(Some(TargetKind::Entity(target)));
        assert_eq!(manager.queued_target, Some(TargetKind::Entity(target)));
        assert_eq!(manager.current_target, None, "SetTarget only queues");
        // Force the update gate open so this test exercises one AI pass.
        manager.update_timer = SpawnTimer::ready();
    }

    tick_spawn_managers(&mut sim, &rules, &[v3], None);

    let manager = sim
        .substrate
        .entities
        .get(v3)
        .and_then(|e| e.spawn_manager.as_ref())
        .expect("manager");
    assert_eq!(manager.current_target, Some(TargetKind::Entity(target)));
    assert_eq!(manager.queued_target, None);
}

#[test]
fn gsi_05_08_hornet_launcher_maximum_accepts_6400_and_clears_6401() {
    let rules = make_spawner_rules();
    let hm = empty_height_map();

    for (distance, expected_mode) in [
        (6400, SpawnManagerMode::Launching),
        (6401, SpawnManagerMode::Idle),
    ] {
        let mut sim = flat_sim();
        let carrier = sim
            .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules, &hm)
            .expect("spawn carrier");
        let target = sim
            .spawn_object("MOBILE", "Yuri", 20, 10, 0, &rules, &hm)
            .expect("spawn initially legal mobile target");
        let manager = sim
            .substrate
            .entities
            .get_mut(carrier)
            .and_then(|entity| entity.spawn_manager.as_mut())
            .expect("carrier manager");
        manager.set_target(Some(TargetKind::Entity(target)));
        manager.update_timer = SpawnTimer::ready();

        move_target_to_x_distance(&mut sim, target, distance);
        tick_spawn_managers(&mut sim, &rules, &[carrier], None);

        let manager = sim
            .substrate
            .entities
            .get(carrier)
            .and_then(|entity| entity.spawn_manager.as_ref())
            .expect("carrier manager after update");
        assert_eq!(manager.mode, expected_mode, "distance {distance}");
        let expected_target = (distance == 6400).then_some(TargetKind::Entity(target));
        assert_eq!(
            manager.current_target, expected_target,
            "distance {distance}"
        );
        assert_eq!(manager.queued_target, None, "distance {distance}");
    }
}

#[test]
fn gsi_05_08_idle_legality_uses_effective_3d_distance() {
    const HORIZONTAL_LEPTONS: i32 = 6000;
    const TARGET_Z_LEPTONS: i32 = 3000;
    const MAX_RANGE_LEPTONS: i64 = 6400;

    assert!(i64::from(HORIZONTAL_LEPTONS) < MAX_RANGE_LEPTONS);
    assert!(
        i64::from(HORIZONTAL_LEPTONS).pow(2) + i64::from(TARGET_Z_LEPTONS).pow(2)
            > MAX_RANGE_LEPTONS.pow(2)
    );

    let rules = make_spawner_rules();
    let hm = empty_height_map();
    let mut sim = flat_sim();
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules, &hm)
        .expect("spawn carrier");
    let target = sim
        .spawn_object("MOBILE", "Yuri", 20, 10, 0, &rules, &hm)
        .expect("spawn initially legal mobile target");
    let manager = sim
        .substrate
        .entities
        .get_mut(carrier)
        .and_then(|entity| entity.spawn_manager.as_mut())
        .expect("carrier manager");
    manager.set_target(Some(TargetKind::Entity(target)));
    manager.update_timer = SpawnTimer::ready();

    move_target_to_x_distance(&mut sim, target, HORIZONTAL_LEPTONS);
    sim.substrate
        .entities
        .get_mut(target)
        .expect("target remains live")
        .position
        .exact_z_leptons = Some(TARGET_Z_LEPTONS);
    tick_spawn_managers(&mut sim, &rules, &[carrier], None);

    let manager = sim
        .substrate
        .entities
        .get(carrier)
        .and_then(|entity| entity.spawn_manager.as_ref())
        .expect("carrier manager after update");
    assert_eq!(manager.mode, SpawnManagerMode::Idle);
    assert_eq!(manager.current_target, None);
    assert_eq!(manager.queued_target, None);
}

#[test]
fn gsi_05_08_v3_minimum_accepts_1280_and_clears_1279() {
    let rules = make_spawner_rules();
    let hm = empty_height_map();

    for (distance, expected_mode) in [
        (1280, SpawnManagerMode::Launching),
        (1279, SpawnManagerMode::Idle),
    ] {
        let mut sim = flat_sim();
        let v3 = sim
            .spawn_object("V3", "Russians", 10, 10, 0, &rules, &hm)
            .expect("spawn V3");
        let target = sim
            .spawn_object("MOBILE", "Yuri", 16, 10, 0, &rules, &hm)
            .expect("spawn initially legal mobile target");
        let manager = sim
            .substrate
            .entities
            .get_mut(v3)
            .and_then(|entity| entity.spawn_manager.as_mut())
            .expect("V3 manager");
        manager.set_target(Some(TargetKind::Entity(target)));
        manager.update_timer = SpawnTimer::ready();

        move_target_to_x_distance(&mut sim, target, distance);
        tick_spawn_managers(&mut sim, &rules, &[v3], None);

        let manager = sim
            .substrate
            .entities
            .get(v3)
            .and_then(|entity| entity.spawn_manager.as_ref())
            .expect("V3 manager after update");
        assert_eq!(manager.mode, expected_mode, "distance {distance}");
        let expected_target = (distance == 1280).then_some(TargetKind::Entity(target));
        assert_eq!(
            manager.current_target, expected_target,
            "distance {distance}"
        );
        assert_eq!(manager.queued_target, None, "distance {distance}");
    }
}

/// Negative half only: proves the gate holds work back before the 20th frame.
/// It does not prove the pass runs *on* the boundary frame, nor that the
/// period becomes 10 afterwards — `set_target_queues_and_the_ai_pass_promotes_it`
/// forces the gate open rather than waiting it out, so the positive edge is
/// **UNCHECKED** by this file.
#[test]
fn update_timer_gates_the_whole_ai_pass() {
    let rules = make_spawner_rules();
    let mut sim = Simulation::new();
    let hm = empty_height_map();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules, &hm)
        .expect("spawn TARGET");
    if let Some(manager) = sim
        .substrate
        .entities
        .get_mut(v3)
        .and_then(|e| e.spawn_manager.as_mut())
    {
        manager.set_target(Some(TargetKind::Entity(target)));
    }

    // Frame 0 with the constructor's 20-frame first delay still pending: no
    // promotion, no launch.
    tick_spawn_managers(&mut sim, &rules, &[v3], None);
    let manager = sim
        .substrate
        .entities
        .get(v3)
        .and_then(|e| e.spawn_manager.as_ref())
        .expect("manager");
    assert_eq!(
        manager.current_target, None,
        "the first AI pass is 20 frames out"
    );
    assert_eq!(manager.mode, SpawnManagerMode::Idle);
}

/// Launch half of the missile cycle: ReadyDocked → InFlight → KamikazeWait,
/// with the flight state and impact payload attached. The slot's later
/// transition into `Regenerating` is NOT covered here — that needs the
/// pause+tilt timer to expire, which this test does not advance.
#[test]
fn v3_launches_its_rocket_into_the_kamikaze_window() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let hm = empty_height_map();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules, &hm)
        .expect("spawn TARGET");
    let child_id = sim
        .substrate
        .entities
        .get(v3)
        .and_then(|e| e.spawn_manager.as_ref())
        .and_then(|m| m.slots[0].spawn)
        .expect("child");

    if let Some(manager) = sim
        .substrate
        .entities
        .get_mut(v3)
        .and_then(|e| e.spawn_manager.as_mut())
    {
        manager.set_target(Some(TargetKind::Entity(target)));
        manager.update_timer = SpawnTimer::ready();
    }
    // Pass 1: Idle → Launching (the slot walk runs before the mode block, so
    // nothing launches while the manager is still Idle).
    tick_spawn_managers(&mut sim, &rules, &[v3], None);
    assert_eq!(
        sim.substrate
            .entities
            .get(v3)
            .and_then(|e| e.spawn_manager.as_ref())
            .map(|m| m.mode),
        Some(SpawnManagerMode::Launching)
    );

    // Pass 2: the slot launches, then the mode block moves it to KamikazeWait.
    if let Some(manager) = sim
        .substrate
        .entities
        .get_mut(v3)
        .and_then(|e| e.spawn_manager.as_mut())
    {
        manager.update_timer = SpawnTimer::ready();
    }
    tick_spawn_managers(&mut sim, &rules, &[v3], None);

    let child = sim.substrate.entities.get(child_id).expect("child alive");
    assert!(!child.lifecycle.in_limbo, "rocket is out in the world");
    assert!(
        child.rocket_state.is_some(),
        "launched missile carries a rocket flight state"
    );
    let payload = child
        .rocket_state
        .as_ref()
        .and_then(|r| r.payload)
        .expect("missile carries its impact payload");
    assert_eq!(payload.damage, 200, "[General] V3RocketDamage");
    assert_eq!(payload.firer_id, v3);

    let manager = sim
        .substrate
        .entities
        .get(v3)
        .and_then(|e| e.spawn_manager.as_ref())
        .expect("manager");
    assert_eq!(manager.slots[0].state, SpawnSlotState::KamikazeWait);
    assert_eq!(
        manager.slots[0].timer.duration, 60,
        "V3RocketPauseFrames + V3RocketTiltFrames"
    );
    assert_eq!(
        manager.count_alive_spawns(),
        1,
        "the slot is still occupied while the missile flies"
    );
}

#[test]
fn a_moving_launcher_holds_its_missile() {
    let rules = make_spawner_rules();
    let mut sim = Simulation::new();
    let hm = empty_height_map();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules, &hm)
        .expect("spawn TARGET");
    let child_id = sim
        .substrate
        .entities
        .get(v3)
        .and_then(|e| e.spawn_manager.as_ref())
        .and_then(|m| m.slots[0].spawn)
        .expect("child");

    if let Some(entity) = sim.substrate.entities.get_mut(v3) {
        // Mid-turn: the native gate is ILocomotor::Is_Moving_Now.
        entity.facing_target = Some(64);
        if let Some(manager) = entity.spawn_manager.as_mut() {
            manager.set_target(Some(TargetKind::Entity(target)));
            manager.update_timer = SpawnTimer::ready();
            manager.mode = SpawnManagerMode::Launching;
        }
    }
    tick_spawn_managers(&mut sim, &rules, &[v3], None);

    assert!(
        sim.substrate
            .entities
            .get(child_id)
            .expect("child")
            .lifecycle
            .in_limbo,
        "a V3 that is still turning may not launch"
    );
}

#[test]
fn missile_impact_kills_through_the_shared_death_pipeline() {
    // The retail contract is that a missile impact runs the same
    // damage -> death -> despawn path as any other detonation. Asserting only
    // "health went down" passes even when nothing handles the kill, so this
    // takes a target the missile can actually destroy and asserts it is gone.
    let rules = make_spawner_rules();
    let mut sim = Simulation::new();
    let hm = empty_height_map();
    let v3 = sim
        .spawn_object("V3", "Soviet", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");
    // FRAGILE (Strength=50) sits well under the V3's 200 damage.
    let target = sim
        .spawn_object("FRAGILE", "Americans", 20, 20, 0, &rules, &hm)
        .expect("spawn FRAGILE");
    let child_id = sim
        .substrate
        .entities
        .get(v3)
        .and_then(|e| e.spawn_manager.as_ref())
        .and_then(|m| m.slots[0].spawn)
        .expect("child");

    // Drive the missile straight to detonation without simulating the flight.
    crate::sim::movement::rocket_movement::attach_rocket_state_with_payload(
        &mut sim.substrate.entities,
        child_id,
        (10, 10),
        (20, 20),
        crate::util::fixed_math::SimFixed::from_num(15),
        Some(crate::sim::movement::rocket_movement::RocketPayload {
            warhead: sim.interner.intern("V3WH"),
            damage: 200,
            firer_id: v3,
        }),
    );
    let _ = sim.reveal(child_id);

    crate::sim::spawn_manager::detonate_missiles(&mut sim, &[child_id]);
    assert_eq!(
        sim.pending_missile_detonations.len(),
        1,
        "the impact is queued for the combat phase, not applied here"
    );
    assert!(
        sim.substrate
            .entities
            .get(child_id)
            .is_none_or(|c| c.dying || !c.lifecycle.object_alive),
        "the missile leaves the world at the detonation moment"
    );

    assert!(
        sim.substrate
            .entities
            .get(target)
            .is_some_and(|t| t.lifecycle.object_alive && t.health.current == 50),
        "fixture guard: the target is still alive before the combat phase runs"
    );

    // One tick: the queued impact is expanded by combat and resolved by the
    // shared death handling.
    sim.advance_tick(&[], Some(&rules), &hm, None, None, 67);
    sim.flush_pending_delete();

    assert!(
        sim.substrate
            .entities
            .get(target)
            .is_none_or(|t| t.dying || !t.lifecycle.object_alive),
        "a target the missile takes to zero must actually die, not stand at 0 HP"
    );
    assert!(
        sim.pending_missile_detonations.is_empty(),
        "the queue is drained after combat"
    );
}
#[test]
fn v3_attack_order_damages_the_target_through_the_spawned_rocket() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let hm = empty_height_map();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");
    let target = sim
        .spawn_object("FRAGILE", "Yuri", 16, 10, 0, &rules, &hm)
        .expect("spawn FRAGILE");

    assert!(
        sim.substrate
            .entities
            .get(target)
            .is_some_and(|t| t.lifecycle.object_alive && t.health.current == 50),
        "fixture guard: the target must start alive at full health"
    );

    if let Some(entity) = sim.substrate.entities.get_mut(v3) {
        entity.attack_target = Some(crate::sim::combat::AttackTarget::new(target));
    }

    // The retail contract is a kill, not a scratch: FRAGILE has Strength=50
    // against the V3's 200 damage, so anything short of the shared death
    // pipeline running leaves it standing at 0 HP and this fails.
    let mut destroyed = false;
    for _ in 0..600 {
        sim.advance_tick(&[], Some(&rules), &hm, None, None, 67);
        let gone = sim
            .substrate
            .entities
            .get(target)
            .is_none_or(|t| t.dying || !t.lifecycle.object_alive);
        if gone {
            destroyed = true;
            break;
        }
    }
    assert!(
        destroyed,
        "a V3 Launcher with an attack order must land its rocket and destroy the target"
    );
    // Kill credit rides the combat damage event, which carries the launcher as
    // the attacker.
}

#[test]
fn owner_death_destroys_docked_children() {
    let rules = make_spawner_rules();
    let mut sim = Simulation::new();
    let hm = empty_height_map();
    let dred = sim
        .spawn_object("DRED", "Russians", 10, 10, 0, &rules, &hm)
        .expect("spawn DRED");
    let children: Vec<u64> = sim
        .substrate
        .entities
        .get(dred)
        .and_then(|e| e.spawn_manager.as_ref())
        .map(|m| m.slots.iter().filter_map(|s| s.spawn).collect())
        .expect("children");
    assert_eq!(children.len(), 2, "SpawnsNumber=2");

    sim.uninit(dred);
    sim.flush_pending_delete();

    for child in children {
        assert!(
            sim.substrate
                .entities
                .get(child)
                .is_none_or(|c| c.dying || !c.lifecycle.object_alive),
            "docked children die with the parent (Kill_All_Spawns)"
        );
    }
}

/// `ObjectClass::ReceiveDamage`'s exact-zero Destroy (`0x005F57AF`) visits the
/// dying owner itself, and its SpawnManager forward (`0x00707B24`) takes the
/// owner arm (`0x006B7CBC`): the docked children are UnInit'd at the killing
/// hit, ahead of the owner's own expiry broadcast and its UnInit.
#[test]
fn a_killing_hit_destroys_docked_children_at_the_destroy() {
    use crate::sim::world::LifecycleTestEvent;

    let rules = make_spawner_rules();
    let mut sim = Simulation::new();
    let hm = empty_height_map();
    let dred = sim
        .spawn_object("DRED", "Russians", 10, 10, 0, &rules, &hm)
        .expect("spawn DRED");
    let children: Vec<u64> = sim
        .substrate
        .entities
        .get(dred)
        .and_then(|e| e.spawn_manager.as_ref())
        .map(|m| m.slots.iter().filter_map(|s| s.spawn).collect())
        .expect("children");
    assert_eq!(children.len(), 2, "SpawnsNumber=2");
    sim.clear_lifecycle_test_events_for_test();

    let warhead = sim.interner.intern("Special");
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        dred,
        10_000,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        warhead,
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(&rules, None, &[hit]);

    let events = sim.lifecycle_test_events_for_test();
    let uninit_of = |id: u64| {
        events.iter().position(|event| {
            matches!(
                event,
                LifecycleTestEvent::UninitRemovalNotifyBoundary { stable_id, .. } if *stable_id == id
            )
        })
    };
    let owner_destroy = events
        .iter()
        .position(|event| *event == LifecycleTestEvent::DestroyNotifyBoundary { stable_id: dred })
        .expect("the killing hit runs the Destroy");
    let owner_uninit = uninit_of(dred).expect("the dead Unit is UnInit'd in the receiver");
    for child in children {
        let child_uninit = uninit_of(child).expect("Kill_All_Spawns UnInits the docked child");
        assert!(
            child_uninit < owner_destroy,
            "the owner arm runs inside the Destroy, before its expiry broadcast"
        );
    }
    assert!(owner_destroy < owner_uninit);
}

#[test]
fn spawn_manager_state_contributes_to_the_state_hash() {
    // The hash folds the pool only when present, so prove the presence and the
    // slot machine both move the hash — otherwise slot/timer divergence would
    // go uncaught in lockstep.
    let rules = make_spawner_rules();
    let hm = empty_height_map();

    let mut a = Simulation::new();
    let v3_a = a
        .spawn_object("V3", "Soviet", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");
    let mut b = Simulation::new();
    let v3_b = b
        .spawn_object("V3", "Soviet", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "identical worlds must hash identically"
    );

    if let Some(manager) = b
        .substrate
        .entities
        .get_mut(v3_b)
        .and_then(|e| e.spawn_manager.as_mut())
    {
        manager.slots[0].state = SpawnSlotState::InFlight;
    }
    assert_ne!(
        a.state_hash(),
        b.state_hash(),
        "a slot-state divergence must show up in the state hash"
    );

    if let Some(entity) = a.substrate.entities.get_mut(v3_a) {
        entity.spawn_manager = None;
    }
    if let Some(entity) = b.substrate.entities.get_mut(v3_b) {
        entity.spawn_manager = None;
    }
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "removing the pool from both sides converges again"
    );
}

/// `Kill_All_Spawns` middle arm: a missile that has already left the launcher
/// (slot in `KamikazeWait`) is removed from the retreat list and destroyed —
/// the salvo does NOT land after the launcher dies.
#[test]
fn launcher_death_destroys_a_missile_already_in_flight() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let hm = empty_height_map();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules, &hm)
        .expect("spawn TARGET");
    let child_id = sim
        .substrate
        .entities
        .get(v3)
        .and_then(|e| e.spawn_manager.as_ref())
        .and_then(|m| m.slots[0].spawn)
        .expect("child");

    // Two forced passes: Idle -> Launching, then launch + KamikazeWait.
    for _ in 0..2 {
        if let Some(manager) = sim
            .substrate
            .entities
            .get_mut(v3)
            .and_then(|e| e.spawn_manager.as_mut())
        {
            manager.set_target(Some(TargetKind::Entity(target)));
            manager.update_timer = SpawnTimer::ready();
        }
        tick_spawn_managers(&mut sim, &rules, &[v3], None);
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(v3)
            .and_then(|e| e.spawn_manager.as_ref())
            .map(|m| m.slots[0].state),
        Some(SpawnSlotState::KamikazeWait),
        "the missile is out and the slot is in its post-launch window"
    );
    assert!(
        !sim.substrate
            .entities
            .get(child_id)
            .expect("missile")
            .lifecycle
            .in_limbo
    );

    sim.uninit(v3);
    sim.flush_pending_delete();

    assert!(
        sim.substrate
            .entities
            .get(child_id)
            .is_none_or(|c| c.dying || !c.lifecycle.object_alive),
        "an in-flight missile dies with its launcher (Kill_All_Spawns state-1 arm)"
    );
}

/// `TechnoClass::ChangeOwner` → `Kill_All_Spawns`: a mind-controlled launcher
/// loses the pool it built for its previous house. The owner is still alive,
/// so the slots re-arm with a zero regen wait rather than the full
/// `SpawnRegenRate`.
#[test]
fn ownership_change_clears_the_pool_and_rearms_without_a_regen_wait() {
    let rules = make_spawner_rules();
    let mut sim = Simulation::new();
    let hm = empty_height_map();
    let dred = sim
        .spawn_object("DRED", "Russians", 10, 10, 0, &rules, &hm)
        .expect("spawn DRED");
    let children: Vec<u64> = sim
        .substrate
        .entities
        .get(dred)
        .and_then(|e| e.spawn_manager.as_ref())
        .map(|m| m.slots.iter().filter_map(|s| s.spawn).collect())
        .expect("children");
    assert_eq!(children.len(), 2);

    let yuri = sim.interner.intern("YuriCountry");
    sim.change_owner(dred, yuri);

    let manager = sim
        .substrate
        .entities
        .get(dred)
        .and_then(|e| e.spawn_manager.as_ref())
        .expect("manager survives the owner change");
    assert!(
        manager
            .slots
            .iter()
            .all(|s| s.state == SpawnSlotState::Regenerating && s.spawn.is_none()),
        "the old owner's pool is gone"
    );
    assert!(
        manager.slots.iter().all(|s| s.timer.duration == 0),
        "an alive owner rebuilds immediately; SpawnRegenRate applies only on death"
    );
    for child in children {
        assert!(
            sim.substrate
                .entities
                .get(child)
                .is_none_or(|c| c.dying || !c.lifecycle.object_alive),
            "docked children of the previous owner are destroyed"
        );
    }
}

/// The missile flies at the RA2-converted `Speed=`, not the raw INI integer.
/// Passing the raw value made a V3 rocket cover roughly a cell per frame.
#[test]
fn missile_flight_speed_uses_the_ra2_conversion() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let hm = empty_height_map();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules, &hm)
        .expect("spawn TARGET");
    let child_id = sim
        .substrate
        .entities
        .get(v3)
        .and_then(|e| e.spawn_manager.as_ref())
        .and_then(|m| m.slots[0].spawn)
        .expect("child");

    for _ in 0..2 {
        if let Some(manager) = sim
            .substrate
            .entities
            .get_mut(v3)
            .and_then(|e| e.spawn_manager.as_mut())
        {
            manager.set_target(Some(TargetKind::Entity(target)));
            manager.update_timer = SpawnTimer::ready();
        }
        tick_spawn_managers(&mut sim, &rules, &[v3], None);
    }

    let speed = sim
        .substrate
        .entities
        .get(child_id)
        .and_then(|c| c.rocket_state.as_ref())
        .map(|r| r.speed)
        .expect("rocket state");
    // V3ROCKET Speed=15 → 15*256/100 = 38 leptons/tick → 38*15 = 570 leptons/s,
    // the unit domain of the six-phase rocket machine (its ascent altitude and
    // acceleration constants are lepton-scale).
    let expected = crate::util::fixed_math::ra2_speed_to_leptons_per_second(15);
    assert_eq!(speed, expected);
    assert_ne!(
        speed,
        crate::util::fixed_math::SimFixed::from_num(15),
        "the raw INI Speed= must not reach the flight-speed field"
    );
    assert_ne!(
        speed,
        crate::util::fixed_math::ra2_speed_to_cells_per_second(15),
        "cells/s is the wrong unit domain for the lepton-scale flight machine \
         (a prior merge briefly fed it, stalling every missile in Ascent)"
    );
}

#[test]
fn gsi_13_07_no_spawn_alt_parser_defaults_false_and_reads_yes() {
    let rules = make_spawner_rules();
    assert!(rules.object("V3").expect("V3 rules").no_spawn_alt);
    assert!(!rules.object("DRED").expect("DRED rules").no_spawn_alt);
}

#[test]
fn gsi_13_07_count_docked_spawns_accepts_only_states_zero_and_six() {
    let rules = make_spawner_rules();
    let mut sim = Simulation::new();
    let hm = empty_height_map();
    let v3 = sim
        .spawn_object("V3", "Soviet", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");

    let manager = sim
        .substrate
        .entities
        .get_mut(v3)
        .and_then(|e| e.spawn_manager.as_mut())
        .expect("V3 spawn manager");

    for (state, expected) in [
        (SpawnSlotState::ReadyDocked, 1),
        (SpawnSlotState::KamikazeWait, 0),
        (SpawnSlotState::InFlight, 0),
        (SpawnSlotState::ReturningToDock, 0),
        (SpawnSlotState::LandingAtDock, 0),
        (SpawnSlotState::Reloading, 1),
        (SpawnSlotState::Regenerating, 0),
    ] {
        manager.slots[0].state = state;
        assert_eq!(manager.count_docked_spawns(), expected, "state {state:?}");
    }
}

#[test]
fn reload_due_restores_actual_and_estimated_health_from_child_type() {
    for strength in [75, 100_000, -7, 0, i32::MAX, i32::MIN] {
        let rules = make_spawner_rules_with_hornet_strength(strength);
        let mut sim = flat_sim();
        let carrier = sim
            .spawn_object(
                "CARRIER",
                "Americans",
                10,
                10,
                0,
                &rules,
                &empty_height_map(),
            )
            .expect("spawn carrier and docked wing");
        let manager = sim
            .substrate
            .entities
            .get_mut(carrier)
            .unwrap()
            .spawn_manager
            .as_mut()
            .unwrap();
        let child = manager.slots[0].spawn.unwrap();
        manager.slots[0].state = SpawnSlotState::Reloading;
        manager.slots[0].timer = SpawnTimer::ready();
        manager.update_timer = SpawnTimer::ready();
        let aircraft = sim.substrate.entities.get_mut(child).unwrap();
        aircraft.health.current = 1;
        aircraft.estimated_health = crate::sim::estimated_health::EstimatedHealth::from_raw(-123);
        let strength = rules.object("HORNET").unwrap().strength;

        tick_spawn_managers(&mut sim, &rules, &[carrier], None);

        let aircraft = sim.substrate.entities.get(child).unwrap();
        assert_eq!(aircraft.health.current, strength);
        assert_eq!(aircraft.estimated_health.get(), strength);
        let manager = sim
            .substrate
            .entities
            .get(carrier)
            .unwrap()
            .spawn_manager
            .as_ref()
            .unwrap();
        assert_eq!(manager.slots[0].state, SpawnSlotState::ReadyDocked);
    }
}

/// A freshly launched Hornet holds station over the deck instead of peeling
/// off at the wing target on its own. Only the manager's Launching block sends
/// the wing out, and only once every slot is committed.
#[test]
fn hornets_hold_over_the_carrier_until_the_whole_wing_is_up() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let hm = empty_height_map();
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules, &hm)
        .expect("spawn CARRIER");
    let target = sim
        .spawn_object("TARGET", "Yuri", 30, 10, 0, &rules, &hm)
        .expect("spawn TARGET");

    // Pass 1: Idle -> Launching. Pass 2: the first Hornet leaves the deck.
    for _ in 0..2 {
        if let Some(manager) = sim
            .substrate
            .entities
            .get_mut(carrier)
            .and_then(|e| e.spawn_manager.as_mut())
        {
            manager.set_target(Some(TargetKind::Entity(target)));
            manager.update_timer = SpawnTimer::ready();
        }
        tick_spawn_managers(&mut sim, &rules, &[carrier], None);
    }

    let launched: Vec<u64> = sim
        .substrate
        .entities
        .get(carrier)
        .and_then(|e| e.spawn_manager.as_ref())
        .map(|m| {
            m.slots
                .iter()
                .filter(|s| s.state == SpawnSlotState::InFlight)
                .filter_map(|s| s.spawn)
                .collect()
        })
        .expect("manager");
    assert!(
        !launched.is_empty(),
        "at least one Hornet should be off the deck"
    );
    for child in &launched {
        assert!(
            sim.substrate
                .entities
                .get(*child)
                .expect("hornet")
                .attack_target
                .is_none(),
            "a Hornet holding formation carries no attack order yet"
        );
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(carrier)
            .and_then(|e| e.spawn_manager.as_ref())
            .map(|m| m.mode),
        Some(SpawnManagerMode::Launching),
        "the manager stays in Launching until every slot is committed"
    );
}

/// A Hornet landing on its Carrier keeps its slot through the dock. The Limbo
/// that docks it broadcasts its expiry (`ObjectClass::Limbo 0x005F4D61`), but
/// the manager's slot arm frees a slot only for a dead child, one on the
/// retreat tracker or a missile slot (`SpawnManagerClass::PointerExpired
/// 0x006B7CDD..0x006B7CF2`). The docked Hornet reloads on `SpawnReloadRate`
/// and is ready again. VERA does not land a recalled Hornet yet (the Fly
/// arrival's BeginLanding call, `0x004CF520`, is unported and the recall Move
/// ends in Idle), so the landing step is staged by hand here.
#[test]
fn a_landing_hornet_keeps_its_slot_and_reloads() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let hm = empty_height_map();
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules, &hm)
        .expect("spawn CARRIER");
    let target = sim
        .spawn_object("TARGET", "Yuri", 30, 10, 0, &rules, &hm)
        .expect("spawn TARGET");
    let tick = |sim: &mut Simulation| {
        if let Some(manager) = sim
            .substrate
            .entities
            .get_mut(carrier)
            .and_then(|e| e.spawn_manager.as_mut())
        {
            manager.update_timer = SpawnTimer::ready();
        }
        tick_spawn_managers(sim, &rules, &[carrier], None);
    };

    // Launch the first Hornet (Idle -> Launching, then off the deck).
    for _ in 0..2 {
        if let Some(manager) = sim
            .substrate
            .entities
            .get_mut(carrier)
            .and_then(|e| e.spawn_manager.as_mut())
        {
            manager.set_target(Some(TargetKind::Entity(target)));
        }
        tick(&mut sim);
    }
    let (slot, hornet) = sim
        .substrate
        .entities
        .get(carrier)
        .and_then(|e| e.spawn_manager.as_ref())
        .and_then(|m| {
            m.slots
                .iter()
                .position(|s| s.state == SpawnSlotState::InFlight)
                .map(|index| (index, m.slots[index].spawn.expect("launched child")))
        })
        .expect("a Hornet is off the deck");
    assert!(
        !sim.substrate
            .entities
            .get(hornet)
            .unwrap()
            .lifecycle
            .in_limbo
    );

    // Bring it home: no wing target, over the deck, on the landing step.
    let deck = sim
        .substrate
        .entities
        .get(carrier)
        .unwrap()
        .position
        .clone();
    {
        let manager = sim
            .substrate
            .entities
            .get_mut(carrier)
            .and_then(|e| e.spawn_manager.as_mut())
            .unwrap();
        manager.set_target(None);
        manager.current_target = None;
        manager.slots[slot].state = SpawnSlotState::LandingAtDock;
    }
    {
        let child = sim.substrate.entities.get_mut(hornet).unwrap();
        child.position = deck;
        if let Some(locomotor) = child.locomotor.as_mut() {
            locomotor.altitude = SimFixed::from_num(0);
        }
    }
    tick(&mut sim);

    let manager = sim
        .substrate
        .entities
        .get(carrier)
        .and_then(|e| e.spawn_manager.as_ref())
        .unwrap();
    assert_eq!(manager.slots[slot].state, SpawnSlotState::Reloading);
    assert_eq!(
        manager.slots[slot].spawn,
        Some(hornet),
        "the Limbo broadcast must not free a living child's slot"
    );
    assert_eq!(
        manager.slots[slot].timer.duration, 150,
        "SpawnReloadRate, not the SpawnRegenRate rebuild"
    );
    assert!(
        sim.substrate
            .entities
            .get(hornet)
            .unwrap()
            .lifecycle
            .in_limbo
    );

    // Reload completes: the same Hornet is ready on the deck again.
    if let Some(manager) = sim
        .substrate
        .entities
        .get_mut(carrier)
        .and_then(|e| e.spawn_manager.as_mut())
    {
        manager.slots[slot].timer = SpawnTimer::ready();
    }
    tick(&mut sim);
    let manager = sim
        .substrate
        .entities
        .get(carrier)
        .and_then(|e| e.spawn_manager.as_ref())
        .unwrap();
    assert_eq!(manager.slots[slot].state, SpawnSlotState::ReadyDocked);
    assert_eq!(manager.slots[slot].spawn, Some(hornet));
}

/// `SpawnManagerClass::PointerExpired`, target arm: the death of the wing's
/// target drops it from the manager. Without this the Carrier cycles back to
/// Launching and sends the whole wing at a corpse, and because the Hornets
/// never fire their ammo never reaches zero, so they never come home.
#[test]
fn target_death_clears_the_wing_target() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let hm = empty_height_map();
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules, &hm)
        .expect("spawn CARRIER");
    let target = sim
        .spawn_object("TARGET", "Yuri", 30, 10, 0, &rules, &hm)
        .expect("spawn TARGET");

    if let Some(manager) = sim
        .substrate
        .entities
        .get_mut(carrier)
        .and_then(|e| e.spawn_manager.as_mut())
    {
        manager.set_target(Some(TargetKind::Entity(target)));
        manager.update_timer = SpawnTimer::ready();
    }
    tick_spawn_managers(&mut sim, &rules, &[carrier], None);
    assert_eq!(
        sim.substrate
            .entities
            .get(carrier)
            .and_then(|e| e.spawn_manager.as_ref())
            .map(|m| (m.current_target, m.mode)),
        Some((
            Some(TargetKind::Entity(target)),
            SpawnManagerMode::Launching
        )),
        "fixture guard: the wing target is live before the kill"
    );

    sim.uninit(target);

    let manager = sim
        .substrate
        .entities
        .get(carrier)
        .and_then(|e| e.spawn_manager.as_ref())
        .expect("manager");
    assert_eq!(
        manager.current_target, None,
        "a destroyed target is dropped the moment it expires"
    );
    assert_eq!(manager.queued_target, None);
    assert_eq!(
        manager.mode,
        SpawnManagerMode::Idle,
        "with no queued replacement the manager falls back to Idle"
    );
}

/// The queued-target arm: expiring a target that is only queued clears just
/// that field and leaves the live one alone.
#[test]
fn queued_target_death_clears_only_the_queued_field() {
    let rules = make_spawner_rules();
    let mut sim = Simulation::new();
    let hm = empty_height_map();
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules, &hm)
        .expect("spawn CARRIER");
    let live = sim
        .spawn_object("TARGET", "Yuri", 30, 10, 0, &rules, &hm)
        .expect("spawn live target");
    let queued = sim
        .spawn_object("TARGET", "Yuri", 31, 10, 0, &rules, &hm)
        .expect("spawn queued target");

    if let Some(manager) = sim
        .substrate
        .entities
        .get_mut(carrier)
        .and_then(|e| e.spawn_manager.as_mut())
    {
        manager.current_target = Some(TargetKind::Entity(live));
        manager.queued_target = Some(TargetKind::Entity(queued));
    }

    sim.uninit(queued);

    let manager = sim
        .substrate
        .entities
        .get(carrier)
        .and_then(|e| e.spawn_manager.as_ref())
        .expect("manager");
    assert_eq!(manager.current_target, Some(TargetKind::Entity(live)));
    assert_eq!(manager.queued_target, None);
}

/// A launch whose target vanished inside the manager window must not leave a
/// revealed child behind. The slot stays docked and the child stays in limbo.
#[test]
fn a_launch_at_a_vanished_target_leaves_no_orphan() {
    let rules = make_spawner_rules();
    let mut sim = Simulation::new();
    let hm = empty_height_map();
    let v3 = sim
        .spawn_object("V3", "Soviet", 10, 10, 0, &rules, &hm)
        .expect("spawn V3");
    let child_id = sim
        .substrate
        .entities
        .get(v3)
        .and_then(|e| e.spawn_manager.as_ref())
        .and_then(|m| m.slots[0].spawn)
        .expect("child");

    // A target id that no longer resolves, written straight past SetTarget so
    // the expiry notification cannot have cleaned it up.
    if let Some(manager) = sim
        .substrate
        .entities
        .get_mut(v3)
        .and_then(|e| e.spawn_manager.as_mut())
    {
        manager.current_target = Some(TargetKind::Entity(999_999));
        manager.mode = SpawnManagerMode::Launching;
        manager.update_timer = SpawnTimer::ready();
    }
    tick_spawn_managers(&mut sim, &rules, &[v3], None);

    let child = sim
        .substrate
        .entities
        .get(child_id)
        .expect("child survives");
    assert!(
        child.lifecycle.in_limbo,
        "nothing is placed in the world when the target cannot resolve"
    );
    assert!(child.rocket_state.is_none());
    assert_eq!(
        sim.substrate
            .entities
            .get(v3)
            .and_then(|e| e.spawn_manager.as_ref())
            .map(|m| m.slots[0].state),
        Some(SpawnSlotState::ReadyDocked),
        "the slot is not committed to InFlight without a flight"
    );
}

/// The manager re-issues `Assign_Target(CurrentTarget)` and `Queue_Mission(
/// Attack, 0)` to an attacking child on every pass (`0x006B7718`,
/// `0x006B772C`). Both are no-ops on a Hornet already running that target
/// (`0x006FCDCC`, `0x005B35E0`), so its pass goes on: VERA used to restart it
/// at state 0 every ten frames, and the wing never finished a pass. A new wing
/// target mid-run is SetTarget's aircraft arm (`0x006FCE27`): the Hornet drops
/// it and its ammo, and the next pass recalls it.
#[test]
fn a_hornet_mid_pass_keeps_its_run_through_the_managers_re_issue() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let hm = empty_height_map();
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules, &hm)
        .expect("spawn CARRIER");
    let target = sim
        .spawn_object("TARGET", "Yuri", 30, 10, 0, &rules, &hm)
        .expect("spawn TARGET");
    let other = sim
        .spawn_object("TARGET", "Yuri", 30, 20, 0, &rules, &hm)
        .expect("spawn the other TARGET");
    let pass = |sim: &mut Simulation, wing_target: Option<u64>| {
        if let Some(manager) = sim
            .substrate
            .entities
            .get_mut(carrier)
            .and_then(|e| e.spawn_manager.as_mut())
        {
            if let Some(id) = wing_target {
                manager.set_target(Some(TargetKind::Entity(id)));
            }
            manager.update_timer = SpawnTimer::ready();
        }
        tick_spawn_managers(sim, &rules, &[carrier], None);
    };
    for _ in 0..2 {
        pass(&mut sim, Some(target));
    }
    // Stage the attacking slot (native status 3) around a launched Hornet
    // three bombs into its run.
    let (slot, hornet) = {
        let manager = sim
            .substrate
            .entities
            .get(carrier)
            .and_then(|e| e.spawn_manager.as_ref())
            .expect("manager");
        manager
            .slots
            .iter()
            .enumerate()
            .find_map(|(i, s)| (s.state == SpawnSlotState::InFlight).then_some((i, s.spawn?)))
            .expect("a launched Hornet")
    };
    sim.substrate
        .entities
        .get_mut(carrier)
        .and_then(|e| e.spawn_manager.as_mut())
        .unwrap()
        .slots[slot]
        .state = SpawnSlotState::ReturningToDock;
    let child = sim.substrate.entities.get_mut(hornet).unwrap();
    let mut attack = crate::sim::combat::AttackTarget::new(target);
    attack.cooldown_ticks = 7;
    child.attack_target = Some(attack);
    child.aircraft_mission = Some(crate::sim::aircraft::AircraftMission::Attack { sub_state: 7 });
    child.aircraft_ammo.as_mut().expect("Hornet ammo").current = 1;

    pass(&mut sim, None);
    let child = sim.substrate.entities.get(hornet).unwrap();
    assert!(matches!(
        child.aircraft_mission,
        Some(crate::sim::aircraft::AircraftMission::Attack { sub_state: 7 })
    ));
    assert_eq!(
        child
            .attack_target
            .as_ref()
            .map(|a| (a.target, a.cooldown_ticks)),
        Some((TargetKind::Entity(target), 7)),
        "the same target is not re-assigned"
    );

    pass(&mut sim, Some(other));
    let child = sim.substrate.entities.get(hornet).unwrap();
    assert_eq!(child.aircraft_ammo.as_ref().unwrap().current, 0);
    assert!(child.attack_target.is_none(), "the aircraft arm drops it");

    pass(&mut sim, None);
    let manager = sim
        .substrate
        .entities
        .get(carrier)
        .and_then(|e| e.spawn_manager.as_ref())
        .unwrap();
    assert_eq!(
        manager.slots[slot].state,
        SpawnSlotState::LandingAtDock,
        "an empty Hornet is recalled"
    );
}

/// Every Hornet's `HornetBomb` frames from a 600-frame Carrier sortie
/// through `advance_tick`, and the wing.
struct StrafingSortie {
    sim: Simulation,
    wing: Vec<u64>,
    bombs: BTreeMap<u64, Vec<u32>>,
}

/// The sortie with strafing Hornets: `HornetBomb` fires a `NormalBomb` (ROT 1,
/// not Inviso), ROF 3. `hornet_keys` extends the fixture's HORNET section. The
/// wing's target is handed to the manager as the Carrier's spawner shot does
/// (`SpawnManagerClass::SetTarget`).
fn strafing_carrier_sortie(hornet_keys: &str) -> StrafingSortie {
    let text = spawner_rules_text()
        .replace(
            "[HORNET]
Name=Hornet
",
            &format!(
                "[HORNET]
Name=Hornet
{hornet_keys}"
            ),
        )
        .replace(
            "[HornetBomb]
Damage=60
ROF=50
Range=4
Projectile=Invisible",
            "[HornetBomb]
Damage=60
ROF=3
Range=5
Projectile=NormalBomb",
        )
        .replace(
            "[Special]",
            "[NormalBomb]
ROT=1
AG=yes

[Special]",
        )
        .replace(
            "[General]
",
            "[General]
CurleyShuffle=yes
",
        );
    let rules = RuleSet::from_ini(&IniFile::from_str(&text)).expect("strafing carrier rules");
    let mut sim = flat_sim();
    let hm = empty_height_map();
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules, &hm)
        .expect("spawn CARRIER");
    let target = sim
        .spawn_object("TARGET", "Yuri", 24, 10, 0, &rules, &hm)
        .expect("spawn TARGET");
    let wing: Vec<u64> = sim
        .substrate
        .entities
        .get(carrier)
        .and_then(|e| e.spawn_manager.as_ref())
        .map(|m| m.slots.iter().filter_map(|s| s.spawn).collect())
        .expect("manager");
    assert_eq!(wing.len(), 3);
    let hornet_bomb = sim.interner.intern("HornetBomb");
    let mut bombs: BTreeMap<u64, Vec<u32>> = BTreeMap::new();
    for _ in 0..600 {
        if let Some(manager) = sim
            .substrate
            .entities
            .get_mut(carrier)
            .and_then(|e| e.spawn_manager.as_mut())
        {
            manager.set_target(Some(TargetKind::Entity(target)));
        }
        sim.fire_events.clear();
        sim.advance_tick(&[], Some(&rules), &hm, None, None, 67);
        let frame = sim.session.binary_frame;
        for event in sim
            .fire_events
            .iter()
            .filter(|e| e.weapon_id == hornet_bomb)
        {
            bombs.entry(event.attacker_id).or_default().push(frame);
        }
    }
    StrafingSortie { sim, wing, bombs }
}

/// Every Hornet of a Carrier wing flies a whole strafe pass: five bombs a
/// weapon ROF apart (Mission_Attack states 4 and 6..9), then its one ammo is
/// paid. The first Hornets up hold over the deck until the whole wing is
/// launched. The fixture Hornet (no `Landable=`) cruises over its hold, and as
/// an armed strafer `0x004D0180` never lets it slow: it circles the hold at
/// full speed. VERA's legacy Fly instead stalled it short of the hold with a
/// zero target speed that no later destination raised again, so only the
/// Hornet launched on the send-out pass ever reached the target. A
/// retail-shaped Hornet (`Landable=`, ROT 3) does not cruise over its hold and
/// slows by distance for it (native lands it there; the Process landing
/// trigger is unported). Either way Fly Process's target speed (`0x004CE145`,
/// `air_movement::write_fly_target_speed`) takes the Hornet to full speed for
/// its run. Before the manager queued Attack in the child's mission owner,
/// `AircraftClass::AI` paid a pass's ammo the frame after its first bomb
/// (`0x0041505E`).
#[test]
fn every_hornet_of_a_carrier_wing_flies_a_whole_strafe_pass() {
    for (shape, hornet_keys) in [("cruising", ""), ("Landable", "Landable=yes\nROT=3\n")] {
        let sortie = strafing_carrier_sortie(hornet_keys);
        for hornet in &sortie.wing {
            let frames = sortie.bombs.get(hornet).map_or(&[][..], Vec::as_slice);
            assert_eq!(
                frames.len(),
                5,
                "{shape} Hornet {hornet}: one pass, five bombs: {:?}",
                sortie.bombs
            );
            for pair in frames.windows(2) {
                assert!(pair[1] - pair[0] >= 3, "a weapon ROF apart: {frames:?}");
            }
            assert_eq!(
                sortie
                    .sim
                    .substrate
                    .entities
                    .get(*hornet)
                    .and_then(|h| h.aircraft_ammo.as_ref())
                    .map(|a| a.current),
                Some(0),
                "{shape} Hornet {hornet}: the pass paid its one ammo"
            );
        }
    }
}
