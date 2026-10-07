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

use crate::map::resolved_terrain::ResolvedTerrainCell;
use crate::rules::ini_parser::IniFile;
use crate::rules::missile_spawn::MissileFamily;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::spawn_manager::{SpawnManagerMode, SpawnSlotState, tick_spawn_managers};
use crate::sim::timer::CdTimer;
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
V3RocketPitchInitial=0.21
V3RocketPitchFinal=0.5
V3RocketTurnRate=0.05
V3RocketRaiseRate=1
V3RocketAcceleration=0.4
V3RocketAltitude=768
V3RocketDamage=200
V3RocketEliteDamage=400
V3RocketBodyLength=256
V3RocketLazyCurve=yes
V3RocketType=V3ROCKET
DMislPauseFrames=20
DMislTiltFrames=60
DMislPitchInitial=0
DMislPitchFinal=0.5
DMislTurnRate=0.08
DMislRaiseRate=1
DMislAcceleration=0.8
DMislAltitude=768
DMislDamage=300
DMislEliteDamage=600
DMislBodyLength=128
DMislLazyCurve=no
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
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
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

fn flat_terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
        base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
        ..crate::map::resolved_terrain::test_tiberium_cell(rx, ry)
    }
}

fn flat_sim() -> Simulation {
    const WIDTH: u16 = 40;
    const HEIGHT: u16 = 32;
    let mut sim = Simulation::new();
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -40,
        off_100: -1,
        off_104: 80,
        off_108: 41,
    });
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
        WIDTH,
        HEIGHT,
        flat_terrain_cell,
    ));
    sim
}

/// [`flat_sim`] with a map `Size=` whose `In_Bounds` diamond (`0x00568300`)
/// holds the V3 tests' cells, as a missile's flight steps require.
fn missile_sim() -> Simulation {
    let mut sim = flat_sim();
    sim.playfield_bounds.as_mut().unwrap().base = 15;
    sim.playfield_size_height = Some(15);
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
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules)
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
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules)
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
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules)
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
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules)
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
        manager.update_timer = CdTimer::default();
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
fn hornet_launcher_maximum_matches_native_distance_ties() {
    let rules = make_spawner_rules();
    // SpawnManager6B7B43 -> Unit+3AC/6F7780 -> CanFireAt6F77B0 ->
    // InRange6F7220. Its approximate distance accepts 6401 at Range6400;
    // the three original numeric controls bound this shared caller check.
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/range_ties.json",
    ))
    .unwrap();
    let rows: Vec<_> = native["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["range"] == 6400)
        .collect();
    assert_eq!(rows.len(), 3);
    for row in rows {
        let distance = (row["input"]["target_xyz"][0].as_i64().unwrap()
            - row["input"]["source_xyz"][0].as_i64().unwrap()) as i32;
        let accepted = row["result"] == 1;
        let expected_mode = if accepted {
            SpawnManagerMode::Launching
        } else {
            SpawnManagerMode::Idle
        };
        let mut sim = flat_sim();
        let carrier = sim
            .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules)
            .expect("spawn carrier");
        let target = sim
            .spawn_object("MOBILE", "Yuri", 20, 10, 0, &rules)
            .expect("spawn initially legal mobile target");
        let manager = sim
            .substrate
            .entities
            .get_mut(carrier)
            .and_then(|entity| entity.spawn_manager.as_mut())
            .expect("carrier manager");
        manager.set_target(Some(TargetKind::Entity(target)));
        manager.update_timer = CdTimer::default();

        move_target_to_x_distance(&mut sim, target, distance);
        tick_spawn_managers(&mut sim, &rules, &[carrier], None);

        let manager = sim
            .substrate
            .entities
            .get(carrier)
            .and_then(|entity| entity.spawn_manager.as_ref())
            .expect("carrier manager after update");
        assert_eq!(manager.mode, expected_mode, "distance {distance}");
        let expected_target = accepted.then_some(TargetKind::Entity(target));
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
    let mut sim = flat_sim();
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules)
        .expect("spawn carrier");
    let target = sim
        .spawn_object("MOBILE", "Yuri", 20, 10, 0, &rules)
        .expect("spawn initially legal mobile target");
    let manager = sim
        .substrate
        .entities
        .get_mut(carrier)
        .and_then(|entity| entity.spawn_manager.as_mut())
        .expect("carrier manager");
    manager.set_target(Some(TargetKind::Entity(target)));
    manager.update_timer = CdTimer::default();

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

    for (distance, expected_mode) in [
        (1280, SpawnManagerMode::Launching),
        (1279, SpawnManagerMode::Idle),
    ] {
        let mut sim = flat_sim();
        let v3 = sim
            .spawn_object("V3", "Russians", 10, 10, 0, &rules)
            .expect("spawn V3");
        let target = sim
            .spawn_object("MOBILE", "Yuri", 16, 10, 0, &rules)
            .expect("spawn initially legal mobile target");
        let manager = sim
            .substrate
            .entities
            .get_mut(v3)
            .and_then(|entity| entity.spawn_manager.as_mut())
            .expect("V3 manager");
        manager.set_target(Some(TargetKind::Entity(target)));
        manager.update_timer = CdTimer::default();

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
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules)
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
/// with the missile's Rocket handed its destination. The slot's later
/// transition into `Regenerating` is NOT covered here — that needs the
/// pause+tilt timer to expire, which this test does not advance.
#[test]
fn v3_launches_its_rocket_into_the_kamikaze_window() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules)
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
        manager.update_timer = CdTimer::default();
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
        manager.update_timer = CdTimer::default();
    }
    tick_spawn_managers(&mut sim, &rules, &[v3], None);

    let child = sim.substrate.entities.get(child_id).expect("child alive");
    assert!(!child.lifecycle.in_limbo, "rocket is out in the world");
    let rocket = child
        .locomotor
        .as_ref()
        .and_then(|locomotor| locomotor.rocket_runtime())
        .expect("the missile flies on its Rocket locomotor");
    // Move_To (0x006632E0) at the target's centre; V3RocketPauseFrames=0
    // starts the tilt.
    assert!(rocket.is_moving());
    assert_eq!(
        rocket.destination_for_test(),
        crate::sim::components::DriveCoord {
            x: 20 * 256 + 128,
            y: 20 * 256 + 128,
            z: 0
        }
    );
    assert_eq!(rocket.mission_state_for_test(), 2);

    let manager = sim
        .substrate
        .entities
        .get(v3)
        .and_then(|e| e.spawn_manager.as_ref())
        .expect("manager");
    assert_eq!(manager.slots[0].state, SpawnSlotState::KamikazeWait);
    assert_eq!(
        manager.slots[0].timer.duration(),
        60,
        "V3RocketPauseFrames + V3RocketTiltFrames"
    );
    assert_eq!(
        manager.count_alive_spawns(),
        1,
        "the slot is still occupied while the missile flies"
    );
}

/// Runs a V3's first manager pass on a still launcher (Idle → Launching,
/// which promotes the queued target), applies `setup` to the launcher, then
/// runs the pass whose slot walk may launch. Returns whether the missile is
/// still in limbo.
fn missile_held_after(setup: impl FnOnce(&mut crate::sim::game_entity::GameEntity, u32)) -> bool {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules)
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
        manager.update_timer = CdTimer::default();
    }
    tick_spawn_managers(&mut sim, &rules, &[v3], None);

    let frame = sim.session.binary_frame;
    let entity = sim.substrate.entities.get_mut(v3).expect("V3");
    setup(entity, frame);
    let manager = entity.spawn_manager.as_mut().expect("manager");
    assert_eq!(manager.mode, SpawnManagerMode::Launching);
    assert!(
        manager.current_target.is_some(),
        "the first pass promoted the target"
    );
    manager.update_timer = CdTimer::default();
    tick_spawn_managers(&mut sim, &rules, &[v3], None);
    sim.substrate
        .entities
        .get(child_id)
        .expect("child")
        .lifecycle
        .in_limbo
}

#[test]
fn a_moving_launcher_holds_its_missile() {
    assert!(
        !missile_held_after(|_, _| {}),
        "a still V3 launches on the second pass"
    );
    // Mid-turn: a Drive's Is_Moving_Now is true while its body turns.
    assert!(
        missile_held_after(|entity, frame| {
            entity.body_facing.set_rot(5);
            entity.body_facing.set(0x4000, frame);
        }),
        "a V3 that is still turning may not launch"
    );
}

/// The missile slot's stationary gate asks the parent's locomotor, Is_Moving
/// (`0x006B731E`) and Is_Moving_Now (`0x006B7349`), not its order: a V3 whose
/// Drive still has a destination holds its missile, and one with an order but
/// a still Drive launches.
#[test]
fn a_launcher_asks_its_locomotor_whether_it_is_moving() {
    assert!(
        missile_held_after(|entity, _| {
            {
                let loco = entity.locomotor.as_mut().unwrap();
                assert!(loco.ensure_installed_track_state());
                assert!(loco.store_track_destination(
                    crate::sim::movement::track_process::TrackFamily::Drive,
                    Some(crate::sim::components::DriveCoord::cell(14, 10, 0))
                ));
            };
        }),
        "a Drive with a destination holds the missile"
    );
    assert!(
        !missile_held_after(|entity, _| {
            entity.movement_target = Some(crate::sim::components::MovementTarget::default());
        }),
        "an order alone does not"
    );
}

/// `Detonate @ 0x00663030` applies its area damage inline: a target the
/// missile takes to zero dies through the shared death handling in the same
/// Process that UnInits the missile.
#[test]
fn missile_impact_kills_through_the_shared_death_pipeline() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let v3 = sim
        .spawn_object("V3", "Soviet", 10, 10, 0, &rules)
        .expect("spawn V3");
    // FRAGILE (Strength=50) sits well under the V3's 200 damage.
    let target = sim
        .spawn_object("FRAGILE", "Americans", 20, 20, 0, &rules)
        .expect("spawn FRAGILE");
    let child_id = sim
        .substrate
        .entities
        .get(v3)
        .and_then(|e| e.spawn_manager.as_ref())
        .and_then(|m| m.slots[0].spawn)
        .expect("child");
    // A cruising missile on the ground beside the target detonates at once.
    let _ = sim.try_reveal_entity(
        child_id,
        crate::sim::world::RevealRequest {
            position: crate::sim::world::RevealPosition {
                exact_z_leptons: Some(0),
                rx: 20,
                ry: 20,
                z: 0,
                sub_x: SimFixed::from_num(128),
                sub_y: SimFixed::from_num(128),
            },
            placement: crate::sim::world::PlacementEvidence::MarkSucceeded,
            logic_eligible: true,
        },
    );
    sim.rocket_move_to(
        child_id,
        crate::sim::components::DriveCoord::cell(20, 20, 0),
        &rules,
    );
    sim.substrate
        .entities
        .get_mut(child_id)
        .and_then(|e| e.locomotor.as_mut())
        .and_then(|locomotor| locomotor.rocket_runtime_mut())
        .expect("rocket")
        .set_mission_state_for_test(4);
    assert!(
        sim.substrate
            .entities
            .get(target)
            .is_some_and(|t| t.lifecycle.object_alive && t.health.current == 50),
        "fixture guard: the target is alive before the impact"
    );

    sim.process_rocket(child_id, &rules, None);

    assert!(
        sim.substrate
            .entities
            .get(child_id)
            .is_none_or(|c| !c.lifecycle.object_alive),
        "the missile leaves the world at the detonation"
    );
    assert!(
        sim.substrate
            .entities
            .get(target)
            .is_none_or(|t| t.dying || !t.lifecycle.object_alive),
        "a target the missile takes to zero must actually die, not stand at 0 HP"
    );
}

#[test]
fn v3_attack_order_damages_the_target_through_the_spawned_rocket() {
    let rules = make_spawner_rules();
    let mut sim = missile_sim();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules)
        .expect("spawn V3");
    let target = sim
        .spawn_object("FRAGILE", "Yuri", 16, 10, 0, &rules)
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
        sim.advance_tick(&[], Some(&rules), None, None, 67);
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
    let dred = sim
        .spawn_object("DRED", "Russians", 10, 10, 0, &rules)
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
    let dred = sim
        .spawn_object("DRED", "Russians", 10, 10, 0, &rules)
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

    let mut a = Simulation::new();
    let v3_a = a
        .spawn_object("V3", "Soviet", 10, 10, 0, &rules)
        .expect("spawn V3");
    let mut b = Simulation::new();
    let v3_b = b
        .spawn_object("V3", "Soviet", 10, 10, 0, &rules)
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
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules)
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
            manager.update_timer = CdTimer::default();
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

/// `ILoco::Process 0x00662FD5..0x00662FE1`: a missile left with no Health in
/// flight (AA's Crash latched it) explodes where it is on its next moving
/// turn (`RocketLocomotion::Detonate 0x00663030`) instead of flying on, so its
/// target is never hit. Shot down as it leaves the rail, it bursts beside its
/// launcher.
#[test]
fn a_missile_shot_down_in_flight_explodes_where_it_is() {
    let rules = make_spawner_rules();
    let mut sim = missile_sim();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules)
        .expect("spawn TARGET");
    let bystander = sim
        .spawn_object("TARGET", "Yuri", 11, 10, 0, &rules)
        .expect("spawn a bystander beside the launcher");
    let missile = sim
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
            manager.update_timer = CdTimer::default();
        }
        tick_spawn_managers(&mut sim, &rules, &[v3], None);
    }
    // The tilt holds the missile still; its climb is the first moving turn.
    let grid = crate::sim::pathfinding::PathGrid::test_all_passable(40, 32);
    let climbing = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(missile)
            .and_then(|m| m.locomotor.as_ref())
            .and_then(|locomotor| locomotor.rocket_runtime())
            .is_some_and(|rocket| rocket.is_moving_now())
    };
    for _ in 0..120 {
        if climbing(&sim) {
            break;
        }
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    }
    assert!(climbing(&sim), "the tilt ends and the missile climbs");
    let entity = sim.substrate.entities.get_mut(missile).expect("missile");
    assert!(!entity.lifecycle.in_limbo);
    entity.health.current = 0;
    entity.crashing = true;
    let target_health = sim.substrate.entities.get(target).unwrap().health.current;

    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);

    assert!(
        sim.substrate
            .entities
            .get(missile)
            .is_none_or(|m| !m.lifecycle.object_alive),
        "the missile explodes and is UnInit"
    );
    assert_eq!(
        sim.substrate.entities.get(target).unwrap().health.current,
        target_health,
        "its payload never reaches the target"
    );
    assert!(
        sim.substrate
            .entities
            .get(bystander)
            .unwrap()
            .health
            .current
            < 1000,
        "the burst lands beside the launcher"
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
    let dred = sim
        .spawn_object("DRED", "Russians", 10, 10, 0, &rules)
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
        manager.slots.iter().all(|s| s.timer.duration() == 0),
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

/// The climb accelerates CurrentSpeed up to the type's `Speed=` in leptons a
/// frame (`0x0066291F..0x00662954`): V3ROCKET Speed=15 caps at 15*256/100.
#[test]
fn missile_flight_speed_uses_the_ra2_conversion() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let v3 = sim
        .spawn_object("V3", "Russians", 10, 10, 0, &rules)
        .expect("spawn V3");
    let target = sim
        .spawn_object("TARGET", "Yuri", 20, 20, 0, &rules)
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
            manager.update_timer = CdTimer::default();
        }
        tick_spawn_managers(&mut sim, &rules, &[v3], None);
    }

    let speed = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(child_id)
            .and_then(|c| c.locomotor.as_ref())
            .and_then(|locomotor| locomotor.rocket_runtime())
            .map(|rocket| rocket.current_speed_for_test())
            .expect("rocket")
    };
    let mut fastest: f64 = 0.0;
    for _ in 0..240 {
        sim.session.binary_frame += 1;
        sim.process_rocket(child_id, &rules, None);
        fastest = fastest.max(speed(&sim));
    }
    assert_eq!(
        fastest,
        f64::from(crate::util::fixed_math::ra2_speed_to_leptons_per_frame(15))
    );
    assert_eq!(fastest, 38.0);
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
    let v3 = sim
        .spawn_object("V3", "Soviet", 10, 10, 0, &rules)
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
            .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules)
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
        manager.slots[0].timer = CdTimer::default();
        manager.update_timer = CdTimer::default();
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
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules)
        .expect("spawn CARRIER");
    let target = sim
        .spawn_object("TARGET", "Yuri", 30, 10, 0, &rules)
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
            manager.update_timer = CdTimer::default();
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

/// `Kill_All_Spawns`' aircraft arm (`0x006B71A2..0x006B71C2`) hands a
/// launched Hornet to `SpawnRetreat__Push @ 0x0054E3B0`, which crashes any
/// child that is not `MissileSpawn=` (`Crash(0)`, `0x0054E3D2`). When its
/// Carrier dies, an airborne Hornet falls instead of flying on, and its slot
/// is regenerating.
#[test]
fn a_dead_carriers_airborne_hornet_crashes() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules)
        .expect("spawn CARRIER");
    let target = sim
        .spawn_object("TARGET", "Yuri", 30, 10, 0, &rules)
        .expect("spawn TARGET");
    for _ in 0..2 {
        if let Some(manager) = sim
            .substrate
            .entities
            .get_mut(carrier)
            .and_then(|e| e.spawn_manager.as_mut())
        {
            manager.set_target(Some(TargetKind::Entity(target)));
            manager.update_timer = CdTimer::default();
        }
        tick_spawn_managers(&mut sim, &rules, &[carrier], None);
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
    // Airborne over the flat level-0 map.
    let entity = sim.substrate.entities.get_mut(hornet).unwrap();
    entity.position.exact_z_leptons = Some(600);
    if let Some(locomotor) = entity.locomotor.as_mut() {
        locomotor.altitude = SimFixed::from_num(600);
    }

    sim.uninit_with_rules(carrier, &rules);

    let hornet = sim
        .substrate
        .entities
        .get(hornet)
        .expect("a crashing Hornet stays represented until its impact");
    assert!(hornet.crashing && hornet.lifecycle.object_alive);
    assert_eq!(hornet.health.current, 0);
    assert_eq!(hornet.spawn_owner_id, None);
    assert!(
        hornet
            .rocking
            .as_ref()
            .is_some_and(|rocking| rocking.vel_sideways != SimFixed::ZERO),
        "Crash drew the spin"
    );
    let manager_slot = sim
        .substrate
        .entities
        .get(carrier)
        .and_then(|e| e.spawn_manager.as_ref())
        .map(|m| (m.slots[slot].spawn, m.slots[slot].state));
    assert!(
        manager_slot
            .is_none_or(|(spawn, state)| spawn.is_none() && state == SpawnSlotState::Regenerating)
    );
}

/// `Kill_All_Spawns` walks the slots from the last (`0x006B7123`), so of two
/// airborne Hornets the one in the higher slot crashes first and takes the
/// first three Scenario draws of `FootClass::Crash` (`RandomRanged(0,
/// 0x7FFFFFFE)` for the sideways rate, `RandomRanged(0, 1)` for its sign,
/// then the forward rate). The sideways magnitude grows with its draw.
#[test]
fn a_dead_carriers_hornets_crash_from_the_last_slot() {
    let rules = make_spawner_rules();
    let mut sim = flat_sim();
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules)
        .expect("spawn CARRIER");
    let target = sim
        .spawn_object("TARGET", "Yuri", 30, 10, 0, &rules)
        .expect("spawn TARGET");
    let airborne = |sim: &Simulation| -> Vec<(usize, u64)> {
        sim.substrate
            .entities
            .get(carrier)
            .and_then(|e| e.spawn_manager.as_ref())
            .map(|m| {
                m.slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.state == SpawnSlotState::InFlight)
                    .filter_map(|(index, s)| Some((index, s.spawn?)))
                    .collect()
            })
            .unwrap_or_default()
    };
    for _ in 0..12 {
        if airborne(&sim).len() >= 2 {
            break;
        }
        if let Some(manager) = sim
            .substrate
            .entities
            .get_mut(carrier)
            .and_then(|e| e.spawn_manager.as_mut())
        {
            manager.set_target(Some(TargetKind::Entity(target)));
            manager.update_timer = CdTimer::default();
            // The launch pacing between Hornets.
            manager.reload_timer = CdTimer::default();
        }
        tick_spawn_managers(&mut sim, &rules, &[carrier], None);
    }
    let wing = airborne(&sim);
    assert!(wing.len() >= 2, "two Hornets off the deck: {wing:?}");
    for &(_, hornet) in &wing {
        let entity = sim.substrate.entities.get_mut(hornet).unwrap();
        entity.position.exact_z_leptons = Some(600);
        if let Some(locomotor) = entity.locomotor.as_mut() {
            locomotor.altitude = SimFixed::from_num(600);
        }
    }
    // The three draws each crash takes, in the order they are taken.
    let mut rng = sim.scenario_rng.clone();
    let draws: Vec<(i32, i32)> = wing
        .iter()
        .map(|_| {
            let sideways = rng.next_range_i32_inclusive(0, 0x7FFF_FFFE);
            let sign = rng.next_range_i32_inclusive(0, 1);
            let _forwards = rng.next_range_i32_inclusive(0, 0x7FFF_FFFE);
            (sideways, sign)
        })
        .collect();

    sim.uninit_with_rules(carrier, &rules);

    // Highest slot first.
    let mut by_slot = wing.clone();
    by_slot.sort_by_key(|&(index, _)| std::cmp::Reverse(index));
    let rates: Vec<SimFixed> = by_slot
        .iter()
        .map(|&(_, hornet)| {
            sim.substrate
                .entities
                .get(hornet)
                .and_then(|e| e.rocking.as_ref())
                .expect("a crashing Hornet spins")
                .vel_sideways
        })
        .collect();
    for (rate, &(_, sign)) in rates.iter().zip(&draws) {
        assert_eq!(*rate < SimFixed::ZERO, sign == 0, "{rates:?} {draws:?}");
    }
    for pair in 0..rates.len() - 1 {
        assert_eq!(
            rates[pair].abs() > rates[pair + 1].abs(),
            draws[pair].0 > draws[pair + 1].0,
            "{rates:?} {draws:?}"
        );
    }
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
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules)
        .expect("spawn CARRIER");
    let target = sim
        .spawn_object("TARGET", "Yuri", 30, 10, 0, &rules)
        .expect("spawn TARGET");
    let tick = |sim: &mut Simulation| {
        if let Some(manager) = sim
            .substrate
            .entities
            .get_mut(carrier)
            .and_then(|e| e.spawn_manager.as_mut())
        {
            manager.update_timer = CdTimer::default();
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
    let deck = sim.substrate.entities.get(carrier).unwrap().position;
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
        manager.slots[slot].timer.duration(),
        150,
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
        manager.slots[slot].timer = CdTimer::default();
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
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules)
        .expect("spawn CARRIER");
    let target = sim
        .spawn_object("TARGET", "Yuri", 30, 10, 0, &rules)
        .expect("spawn TARGET");

    if let Some(manager) = sim
        .substrate
        .entities
        .get_mut(carrier)
        .and_then(|e| e.spawn_manager.as_mut())
    {
        manager.set_target(Some(TargetKind::Entity(target)));
        manager.update_timer = CdTimer::default();
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
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules)
        .expect("spawn CARRIER");
    let live = sim
        .spawn_object("TARGET", "Yuri", 30, 10, 0, &rules)
        .expect("spawn live target");
    let queued = sim
        .spawn_object("TARGET", "Yuri", 31, 10, 0, &rules)
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
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules)
        .expect("spawn CARRIER");
    let target = sim
        .spawn_object("TARGET", "Yuri", 30, 10, 0, &rules)
        .expect("spawn TARGET");
    let other = sim
        .spawn_object("TARGET", "Yuri", 30, 20, 0, &rules)
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
            manager.update_timer = CdTimer::default();
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
    let frame = sim.session.binary_frame as i32;
    let child = sim.substrate.entities.get_mut(hornet).unwrap();
    child.attack_target = Some(crate::sim::combat::AttackTarget::new(target));
    child.rearm_timer = crate::sim::timer::CdTimer::started(frame, 7);
    child.aircraft_mission = Some(crate::sim::aircraft::AircraftMission::Attack { sub_state: 7 });
    child.aircraft_ammo.as_mut().expect("Hornet ammo").current = 1;

    pass(&mut sim, None);
    let child = sim.substrate.entities.get(hornet).unwrap();
    assert!(matches!(
        child.aircraft_mission,
        Some(crate::sim::aircraft::AircraftMission::Attack { sub_state: 7 })
    ));
    assert_eq!(
        child.attack_target.as_ref().map(|a| a.target),
        Some(TargetKind::Entity(target)),
        "the same target is not re-assigned"
    );
    assert_eq!(
        child.rearm_timer,
        crate::sim::timer::CdTimer::started(frame, 7)
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
    let carrier = sim
        .spawn_object("CARRIER", "Americans", 10, 10, 0, &rules)
        .expect("spawn CARRIER");
    let target = sim
        .spawn_object("TARGET", "Yuri", 24, 10, 0, &rules)
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
        sim.advance_tick(&[], Some(&rules), None, None, 67);
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

/// One retail launch, read when the slot's AI pass returned: the child's
/// Location, exact Z and GetHeight, and the owner's Location, body heading,
/// its rounded direction (the one the child's Unlimbo takes) and burst index.
struct RetailLaunch {
    missile: crate::sim::components::DriveCoord,
    missile_exact_z: Option<i32>,
    missile_height: i32,
    owner: crate::sim::components::DriveCoord,
    owner_facing: u16,
    owner_dir: u8,
    owner_burst: i32,
}

/// A retail `owner_type` on flat level-4 ground, its body at DirType 0x20 and
/// its burst index at 1 (a burst in progress), launches every slot at a cell,
/// one manager pass per slot. This is the pose the original's case-0 rows
/// were run on (tools/projectile_oracle/ifv_fire_coord.json `spawn_launch`).
fn retail_launches(
    rules: &RuleSet,
    owner_type: &str,
) -> (Simulation, u64, Vec<(u64, RetailLaunch)>) {
    use crate::sim::movement::ground_pose::position_world_coord;

    let mut sim = Simulation::new();
    let house = sim.interner.intern("Russians");
    sim.houses.insert(
        house,
        crate::sim::house_state::HouseState::new(house, 0, None, true, 0, 10),
    );
    sim.session.house_order.push(house);
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    for y in 0..32 {
        for x in 0..32 {
            terrain.cell_mut(x, y).unwrap().level = 4;
        }
    }
    // MarkSucceeded seats a ship on this dry fixture ground.
    let owner = sim
        .construct_object_limbo_at_height(owner_type, "Russians", 10, 10, 0x20, 4, rules)
        .expect("construct owner");
    assert!(
        sim.reveal_constructed_object_at_height(
            owner,
            10,
            10,
            0x20,
            4,
            crate::sim::world::PlacementEvidence::MarkSucceeded,
            rules,
        )
        .is_some()
    );
    let entity = sim.substrate.entities.get_mut(owner).unwrap();
    entity.weapon_burst.complete_shot(2);
    assert_eq!(entity.weapon_burst.index(), 1);
    let manager = entity.spawn_manager.as_mut().expect("missile pool");
    manager.current_target = Some(TargetKind::Cell(20, 10));
    manager.mode = SpawnManagerMode::Launching;
    let slots = manager.slots.len();

    let mut launches = Vec::new();
    for slot in 0..slots {
        let manager = sim
            .substrate
            .entities
            .get_mut(owner)
            .and_then(|e| e.spawn_manager.as_mut())
            .unwrap();
        manager.update_timer = CdTimer::default();
        manager.reload_timer = CdTimer::default();
        tick_spawn_managers(&mut sim, rules, &[owner], None);
        let frame = sim.session.binary_frame;
        let owner_entity = sim.substrate.entities.get(owner).unwrap();
        let child = owner_entity.spawn_manager.as_ref().unwrap().slots[slot]
            .spawn
            .expect("the slot keeps its missile");
        let missile = sim.substrate.entities.get(child).unwrap();
        assert!(!missile.lifecycle.in_limbo, "slot {slot} launched");
        launches.push((
            child,
            RetailLaunch {
                missile: position_world_coord(&missile.position),
                missile_exact_z: missile.position.exact_z_leptons,
                missile_height: crate::sim::movement::air_movement::current_fly_height(
                    missile,
                    sim.resolved_terrain.as_ref(),
                ),
                owner: position_world_coord(&owner_entity.position),
                owner_facing: owner_entity.body_facing.current(frame),
                owner_dir: owner_entity.body_facing_dir(frame),
                owner_burst: owner_entity.weapon_burst.index(),
            },
        ));
    }
    (sim, owner, launches)
}

/// `SpawnManagerClass::AI` case 0 (`0x006B73C4..0x006B7585`) against the
/// original's runs (tools/projectile_oracle/ifv_fire_coord.json
/// `spawn_launch.launches`), through the production readers on retail
/// RULESMD/ARTMD: each missile unlimbos at the coordinate and direction the
/// original hands its Unlimbo, Reveal keeps that coordinate as its Location
/// and exact Z, and the owner's burst index ends as the original's. The rows
/// cover V3 (weapon 0, Burst=1, odd owner index), both DRED slots (Burst=2
/// parity) and both BSUB slots (weapon 1, `SecondSpawnOffset=` on the odd
/// slot, `CMislType=`).
#[test]
fn retail_missiles_keep_their_launch_coordinate_from_unlimbo() {
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::flh::Flh;
    use crate::sim::components::DriveCoord;

    let Some((ini, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/ifv_fire_coord.json",
    ))
    .unwrap();
    let native = &native["spawn_launch"];
    let int = |value: &serde_json::Value| value.as_i64().unwrap() as i32;
    let coord = |value: &serde_json::Value| DriveCoord {
        x: int(&value[0]),
        y: int(&value[1]),
        z: int(&value[2]),
    };
    let flh =
        |value: &serde_json::Value| Flh::from([int(&value[0]), int(&value[1]), int(&value[2])]);
    // The production readers agree with the original's reads of the inputs.
    for (name, row) in native["types"].as_object().unwrap() {
        let art = rules.art().get(name).unwrap();
        assert_eq!(art.primary_fire_flh, flh(&row["weapon_flh"][0]), "{name}");
        assert_eq!(art.secondary_fire_flh, flh(&row["weapon_flh"][1]), "{name}");
        assert_eq!(
            art.second_spawn_offset,
            flh(&row["second_spawn_offset"]),
            "{name}"
        );
    }
    for (name, row) in native["weapons"].as_object().unwrap() {
        let weapon = rules.weapon(name).unwrap();
        assert_eq!(weapon.burst, int(&row["burst"]), "{name}");
        assert_eq!(weapon.spawner, row["spawner"].as_bool().unwrap(), "{name}");
    }
    assert_eq!(rules.missile_spawn.cmisl.type_name, "CMISL");
    let origin = coord(&native["supplied"]["origin"]);
    let heading = native["supplied"]["primary_and_secondary_heading"]
        .as_u64()
        .unwrap() as u16;

    for owner_type in ["V3", "DRED", "BSUB"] {
        let (sim, _, launches) = retail_launches(&rules, owner_type);
        let rows: Vec<_> = native["launches"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["owner"] == owner_type)
            .collect();
        assert_eq!(launches.len(), rows.len(), "{owner_type}");
        for ((_, launch), row) in launches.iter().zip(&rows) {
            assert_eq!(int(&row["burst_before"]), 1, "the fixture's burst index");
            assert_eq!((launch.owner, launch.owner_facing), (origin, heading));
            let expected = coord(&row["unlimbo_coordinate"]);
            assert_eq!(launch.missile, expected, "{row}");
            assert_eq!(launch.missile_exact_z, Some(expected.z), "{row}");
            assert_eq!(
                launch.missile_height,
                expected.z - origin.z,
                "{row}: the owner stands on the floor"
            );
            // The rocket's flight turns the child afterwards; the direction
            // its Unlimbo took is the owner's, rounded.
            assert_eq!(i32::from(launch.owner_dir), int(&row["unlimbo_direction"]));
            assert_eq!(launch.owner_burst, int(&row["burst_after"]), "{row}");
        }
        for (missile, _) in &launches {
            let rocket = sim
                .substrate
                .entities
                .get(*missile)
                .and_then(|m| m.locomotor.as_ref())
                .and_then(|locomotor| locomotor.rocket_runtime())
                .expect("the missile flies on its Rocket locomotor");
            assert!(rocket.is_moving(), "{owner_type}: Move_To took the target");
        }
    }
}
