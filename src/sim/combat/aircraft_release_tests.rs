use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::docking::aircraft_dock::AircraftAmmo;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;
use crate::sim::mission::leaf::MissionLeafState;
use crate::sim::movement::{FacingClass, locomotor::LocomotorState};
use serde_json::Value;

fn fixture(input: &Value) -> (Simulation, RuleSet) {
    let elite = input["elite_weapon"].as_bool().unwrap_or(false);
    let passenger = input["passenger"].as_bool().unwrap_or(false);
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[AircraftTypes]\n0=ORCA\n[ORCA]\nStrength=150\nSpeed=8\nAmmo=2\nROT=5\n\
         Primary=Gun\n{}Fighter={}\nLocomotor={{4A582746-9839-11d1-B709-00A024DDAFD1}}\n{}\
         [Gun]\nDamage=10\nROF=20\nRange=20\nBurst={}\nOmniFire={}\nProjectile=Shell\nWarhead=WH\n{}\
         [EliteGun]\nDamage=10\nROF=37\nRange=20\nBurst={}\nProjectile=Shell\nWarhead=WH\n\
         [Shell]\nROT={}\nInviso={}\nAG=yes\nAA=yes\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n{}{}",
        if elite { "ElitePrimary=EliteGun\n" } else { "" },
        input["fighter"].as_bool().unwrap_or(false),
        if input["missile_spawn"].as_bool().unwrap_or(false) {
            "MissileSpawn=yes\n"
        } else {
            ""
        },
        input["burst"].as_i64().unwrap_or(2),
        input["omni_fire"].as_bool().unwrap_or(false),
        input["speed"]
            .as_i64()
            .map_or(String::new(), |speed| format!("Speed={speed}\n")),
        input["elite_burst"].as_i64().unwrap_or(3),
        input["rot"].as_i64().unwrap_or(3),
        input["inviso"].as_bool().unwrap_or(true),
        if passenger {
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n"
        } else {
            ""
        },
        input["extra_ini"].as_str().unwrap_or(""),
    )))
    .unwrap();
    let mut sim = Simulation::with_seed(0);
    let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
    entity.set_category_for_test(EntityCategory::Aircraft);
    entity.mission_leaf = MissionLeafState::aircraft_raw_for_test(0, 1, false);
    entity
        .mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: crate::sim::mission::MissionId::from_raw(1),
            suspended: crate::sim::mission::MissionId::NONE,
            queued: crate::sim::mission::MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 4,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
        });
    entity.aircraft_ammo = Some(AircraftAmmo::new(2));
    entity.aircraft_ammo.as_mut().unwrap().current = input["ammo"].as_i64().unwrap_or(2) as i32;
    entity
        .veterancy
        .set_rank_for_test((input["veterancy"].as_u64().unwrap_or(0) * 100) as u16);
    entity.body_facing = FacingClass::new(0, 5);
    entity.barrel_facing = Some(FacingClass::new(0, 5));
    entity
        .attack_target
        .set_for_test(Some(TargetKind::Cell(10, 9)));
    entity.locomotor = Some(LocomotorState::from_object_type(
        rules.object("ORCA").unwrap(),
        0,
    ));
    if passenger {
        let mut cargo = crate::sim::passenger::PassengerCargo::new(8, 0);
        cargo.board_forced(2, 1);
        entity.passenger_role = crate::sim::passenger::PassengerRole::transport(cargo);
    }
    sim.substrate.entities.insert(entity);
    sim.substrate.next_stable_object_id = 2;
    sim.interner = test_interner();
    if passenger {
        // A playfield holding the aircraft's cell (10, 10).
        sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
            base: 10,
            off_fc: 0,
            off_100: 0,
            off_104: 60,
            off_108: 60,
        });
        let mut infantry = GameEntity::test_default(2, "E1", "Americans", 10, 10);
        infantry.set_owner_for_test(sim.interner.intern("Americans"));
        infantry.set_type_ref_for_test(sim.interner.intern("E1"));
        infantry.set_category_for_test(EntityCategory::Infantry);
        infantry.is_voxel = false;
        infantry.sub_cell = Some(2);
        infantry.passenger_role = crate::sim::passenger::PassengerRole::inside_for_test(1, false);
        sim.substrate.entities.insert(infantry);
        sim.substrate.next_stable_object_id = 3;
    }
    sim.set_logic_order_for_test(vec![1]);
    (sim, rules)
}

fn dispatch(sim: &mut Simulation, rules: &RuleSet) -> CombatTickResult {
    let requests = crate::sim::combat::FireRequests {
        aircraft: crate::sim::aircraft::tick_aircraft_missions(sim, rules),
    };
    let mut run = ReceiverRun::default();
    tick_combat(
        sim,
        &mut run,
        rules,
        None,
        100,
        &[1],
        &BTreeSet::new(),
        &requests,
        &[],
    )
}

#[test]
fn aircraft_release_control_matches_316_original_mission_suffixes() {
    // Native witness replaces FireAt with a NULL-return callback. Here actual
    // shared emission runs; only loop count, pending ammo and suffix are parity
    // assertions. Projectile math and GetROF are outside that native witness.
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/aircraft_attack_release.json",
    ))
    .unwrap();
    let rows = corpus["releases"].as_array().unwrap();
    assert_eq!(rows.len(), 316);
    for row in rows {
        let (mut sim, rules) = fixture(&row["input"]);
        let result = dispatch(&mut sim, &rules);
        let entity = sim.substrate.entities.get(1).unwrap();
        assert!(
            entity.aircraft_ammo.as_ref().unwrap().release_pending(),
            "{row}"
        );
        assert_eq!(
            entity.aircraft_ammo.as_ref().unwrap().current,
            row["input"]["ammo"].as_i64().unwrap() as i32,
            "{row}"
        );
        let Some(sub_state) = crate::sim::aircraft::attack_state(entity) else {
            panic!("{row}")
        };
        assert_eq!(sub_state as u64, row["state"].as_u64().unwrap(), "{row}");
        assert_eq!(
            entity.mission.dispatch_timer().delay() as i64,
            row["delay"].as_i64().unwrap(),
            "{row}"
        );
        assert_eq!(
            entity.mission_leaf.as_aircraft().unwrap().action_latch() != 0,
            row["latch_6d2"].as_bool().unwrap(),
            "{row}"
        );
        let calls = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["call"] == "fire")
            .count();
        assert_eq!(result.consequences.fire_events().len(), calls, "{row}");
        assert_eq!(entity.weapon_burst.index(), 0, "{row}");
    }
}

#[test]
fn aircraft_request_preserves_rearm_and_state3_does_not_fire_early() {
    let (mut sim, rules) = fixture(&serde_json::json!({"burst":2,"fighter":true}));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .mission
        .set_current_for_test(
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Attack),
            3,
        );
    assert!(
        dispatch(&mut sim, &rules)
            .consequences
            .fire_events()
            .is_empty()
    );
    let frame = sim.session.binary_frame as i32;
    sim.substrate.entities.get_mut(1).unwrap().rearm_timer =
        crate::sim::timer::CdTimer::started(frame, 4);
    assert!(
        dispatch(&mut sim, &rules)
            .consequences
            .fire_events()
            .is_empty()
    );
    // The request leaves the object's own reload running.
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .rearm_timer
            .remaining(frame),
        4
    );
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .aircraft_ammo
            .as_ref()
            .unwrap()
            .release_pending()
    );
}

#[test]
fn aircraft_release_uses_raw_burst_above_byte_width_and_one_ammo_charge() {
    let (mut sim, rules) = fixture(&serde_json::json!({"burst":257,"fighter":true}));
    let result = dispatch(&mut sim, &rules);
    assert_eq!(result.consequences.fire_events().len(), 257);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.aircraft_ammo.as_ref().unwrap().current, 2);
    assert_eq!(entity.weapon_burst.index(), 0);
    // The Mission return owns cadence, independent of the final rearm jitter.
    sim.session.binary_frame = 19;
    assert!(crate::sim::aircraft::tick_aircraft_missions(&mut sim, &rules).is_empty());
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .aircraft_ammo
            .as_ref()
            .unwrap()
            .current,
        2
    );
    sim.session.binary_frame = 20;
    crate::sim::aircraft::tick_aircraft_missions(&mut sim, &rules);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.aircraft_ammo.as_ref().unwrap().current, 1);
    assert!(!entity.aircraft_ammo.as_ref().unwrap().release_pending());
}

#[test]
fn aircraft_release_snapshot_retains_burst_pending_and_mission_delay() {
    use crate::sim::snapshot::GameSnapshot;
    let (mut sim, rules) = fixture(&serde_json::json!({"burst":3,"fighter":true}));
    let before = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .weapon_burst
        .complete_shot(3);
    assert_ne!(before, sim.state_hash());
    assert_eq!(
        dispatch(&mut sim, &rules).consequences.fire_events().len(),
        3
    );
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().weapon_burst.index(),
        1
    );
    let saved = GameSnapshot::save(&sim, 0, 0, "admitted aircraft burst", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    // Snapshot's existing RNG-load policy initializes seed0; this test checks
    // retained release state and its no-RNG next entry, not that other policy.
    restored.scenario_rng = sim.scenario_rng.clone();
    assert_eq!(restored.state_hash(), sim.state_hash());
    for world in [&mut sim, &mut restored] {
        world.session.binary_frame = 20;
        crate::sim::aircraft::tick_aircraft_missions(world, &rules);
        let entity = world.substrate.entities.get(1).unwrap();
        assert_eq!(entity.weapon_burst.index(), 1);
        assert_eq!(entity.aircraft_ammo.as_ref().unwrap().current, 1);
        assert!(!entity.aircraft_ammo.as_ref().unwrap().release_pending());
    }
    assert_eq!(restored.state_hash(), sim.state_hash());
}

#[test]
fn aircraft_release_runs_through_advance_tick() {
    let (mut sim, rules) = fixture(&serde_json::json!({"burst":2,"fighter":true}));
    sim.set_logic_order_for_test(vec![1]);
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(sim.fire_events.len(), 2);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.aircraft_ammo.as_ref().unwrap().release_pending());
    assert_eq!(entity.aircraft_ammo.as_ref().unwrap().current, 2);
    assert_eq!(crate::sim::aircraft::attack_state(entity), Some(1));
}

#[test]
fn aircraft_secondary_arc_ignores_homing_and_omnifire_but_fighter_bypasses_it() {
    // Aircraft41AA22..41AA6A always reads SecondaryFacing, with inclusive0x800.
    // Check that shared admission cannot substitute hull, homing tolerance or
    // OmniFire, and that state4's Set calls retain their interpolated current.
    for fighter in [false, true] {
        for delta in [0x800, 0x801, 0xFFFF] {
            let (mut sim, rules) = fixture(&serde_json::json!({
                "burst":1,"fighter":fighter,"rot":3,"omni_fire":true
            }));
            sim.substrate.entities.get_mut(1).unwrap().barrel_facing =
                Some(FacingClass::new(delta, 5));
            let result = dispatch(&mut sim, &rules);
            assert_eq!(
                result.consequences.fire_events().len(),
                usize::from(fighter || delta != 0x801),
                "fighter={fighter} delta={delta}"
            );
        }
    }
}

/// One strafe pass (`0x00417FE0` states 4 and 6..9): the state-4 release and
/// one bomb each in states 6, 7, 8 and 9, each visit a weapon-0 ROF after the
/// last (a still-running rearm timer polls a frame at a time, `0x00418BC5`).
/// State 9 hands off to state 3 after `(Range + 0x400) / speed` frames, and
/// state 3 pays the pass's one pending ammo. VERA used to drop one bomb.
#[test]
fn a_strafer_drops_five_bombs_on_one_pass() {
    let (mut sim, rules) = fixture(&serde_json::json!({
        "burst": 1, "fighter": false, "rot": 1, "inviso": false, "ammo": 1
    }));
    let mut bombs = Vec::new();
    let mut states = vec![4u8];
    for frame in 0..400u32 {
        sim.session.binary_frame = frame;
        let fired = dispatch(&mut sim, &rules).consequences.fire_events().len();
        if fired > 0 {
            bombs.push((frame, fired));
        }
        let Some(sub_state) =
            crate::sim::aircraft::attack_state(sim.substrate.entities.get(1).unwrap())
        else {
            break;
        };
        if states.last() != Some(&sub_state) {
            states.push(sub_state);
        }
        if sub_state == 3 {
            break;
        }
    }
    assert_eq!(bombs.iter().map(|&(_, n)| n).sum::<usize>(), 5, "{bombs:?}");
    assert_eq!(states, vec![4, 6, 7, 8, 9, 3], "{bombs:?}");
    for pair in bombs.windows(2) {
        assert!(
            pair[1].0 - pair[0].0 >= 20,
            "a weapon-0 ROF apart: {bombs:?}"
        );
    }
    let entity = sim.substrate.entities.get(1).unwrap();
    let ammo = entity.aircraft_ammo.as_ref().unwrap();
    assert!(ammo.release_pending(), "state 3 pays it");
    assert_eq!(ammo.current, 1);
    // (Range 20 cells + 0x400) / (Speed 8 -> 20 leptons a frame).
    assert_eq!(
        entity.mission.dispatch_timer().delay(),
        (20 * 256 + 0x400) / 20
    );
}

/// A Fighter out of range (`0x00418544` then `0x0041874E`): state 4's
/// refusal sends it to state 5 on the next frame, and state 5, not close,
/// back to state 1 after the Attack Rate and one Scenario draw. No shot.
#[test]
fn a_fighter_out_of_range_cycles_back_to_its_search() {
    let (mut sim, rules) = fixture(&serde_json::json!({"burst": 1, "fighter": true}));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Cell(10, 60)));
    assert!(
        dispatch(&mut sim, &rules)
            .consequences
            .fire_events()
            .is_empty()
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(crate::sim::aircraft::attack_state(entity), Some(5));
    assert_eq!(entity.mission.dispatch_timer().delay(), 1);

    let before = sim.scenario_rng.clone();
    sim.session.binary_frame = 1;
    assert!(
        dispatch(&mut sim, &rules)
            .consequences
            .fire_events()
            .is_empty()
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(crate::sim::aircraft::attack_state(entity), Some(1));
    let mut expected = before;
    let rate = rules
        .mission_control
        .rate_frames(crate::sim::mission::MissionType::Attack);
    assert_eq!(
        entity.mission.dispatch_timer().delay(),
        rate + expected.next_range_i32_inclusive(0, 2)
    );
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

/// The first bullet a strike releases.
fn first_bullet(sim: &mut Simulation, rules: &RuleSet) -> crate::sim::projectile::Projectile {
    for frame in 0..60u32 {
        sim.session.binary_frame = frame;
        dispatch(sim, rules);
        if let Some((_, bullet)) = sim.projectiles.iter().next() {
            return bullet.clone();
        }
    }
    panic!("the strike releases no bullet");
}

/// `AircraftClass::Fire_At`'s ROT 1 course (`0x004160CF..0x00416318`): a
/// strafer's bomb leaves at weapon 0's speed (`+0xA8`, `Speed=30` read as 76
/// leptons a frame), aimed from the aircraft at its target, the cell north
/// of it. TechnoClass::FireAt alone launches a ROT 1 bullet at speed 1.
#[test]
fn a_strafers_bomb_leaves_at_its_weapon_speed_toward_the_target() {
    let (mut sim, rules) = fixture(&serde_json::json!({
        "burst": 1, "rot": 1, "inviso": false, "ammo": 1, "speed": 30
    }));
    let weapon_speed = rules.weapon("Gun").unwrap().speed;
    assert_eq!(weapon_speed, 76);
    let bomb = first_bullet(&mut sim, &rules);
    let [x, y, z] = bomb
        .velocity
        .native()
        .map(|axis| f64::from_bits(axis.bits()));
    let speed = (x * x + y * y + z * z).sqrt();
    assert!((speed - 76.0).abs() < 0.5, "at 76: ({x}, {y}, {z})");
    assert!(
        y < -75.0 && x.abs() < 1.0,
        "north, at the target: ({x}, {y}, {z})"
    );
}

/// `0x00415EEE..0x00415F05`: an aircraft carrying a passenger drops it
/// (`Drop_Payload`) where it would fire, and its strike carries on as after
/// a shot. VERA used to hold such an aircraft in state 4 with no shot.
#[test]
fn an_aircraft_carrying_a_passenger_drops_it_instead_of_firing() {
    let (mut sim, rules) = fixture(&serde_json::json!({"burst": 1, "passenger": true}));
    let result = dispatch(&mut sim, &rules);
    assert!(result.consequences.fire_events().is_empty(), "no shot");
    assert!(sim.projectiles.is_empty(), "no bullet");
    let aircraft = sim.substrate.entities.get(1).unwrap();
    assert!(aircraft.passenger_role.cargo().unwrap().is_empty());
    assert_eq!(crate::sim::aircraft::attack_state(aircraft), Some(5));
    let dropped = sim.substrate.entities.get(2).unwrap();
    assert!(!dropped.passenger_role.is_inside_transport(), "dropped");
}

/// `0x00418506`: state 4 reads Ammo again after the release, so a fighter
/// whose last Ammo went with its passenger (Drop_Payload spends one) leaves
/// for state 10 instead of coming round again.
#[test]
fn a_fighter_that_drops_its_last_ammo_with_a_passenger_leaves() {
    let (mut sim, rules) = fixture(&serde_json::json!({
        "burst": 1,
        "passenger": true,
        "fighter": true,
        "ammo": 1,
    }));
    dispatch(&mut sim, &rules);
    let aircraft = sim.substrate.entities.get(1).unwrap();
    assert!(
        aircraft.passenger_role.cargo().unwrap().is_empty(),
        "dropped"
    );
    assert_eq!(aircraft.aircraft_ammo.as_ref().unwrap().current, 0);
    assert_eq!(crate::sim::aircraft::attack_state(aircraft), Some(10));
}

/// `0x0041631F..0x00416595`: a shot by a human player's aircraft flying at
/// 1500 leptons over shrouded flat ground. IsShrouded (`0x00586360`) asks
/// the Cell its height raises the Location onto, 7 cells up the map, and
/// RevealArea maps `AttackingAircraftSightRange=` cells around that raised
/// centre on that player's map, not around the ground under the aircraft.
/// A computer's shot maps nothing.
#[test]
fn a_human_aircraft_shooting_over_shroud_maps_the_cells_around_it() {
    for human in [false, true] {
        let (mut sim, mut rules) = fixture(&serde_json::json!({"burst": 1}));
        rules.general.attacking_aircraft_sight_range = 2;
        sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
            64,
            64,
            crate::map::resolved_terrain::test_flat_cell,
        ));
        sim.fog.width = 64;
        sim.fog.height = 64;
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .position
            .exact_z_leptons = Some(1500);
        let americans = sim.interner.intern("Americans");
        sim.houses.insert(
            americans,
            crate::sim::house_state::HouseState::new(americans, 0, None, human, 0, 10),
        );
        let result = dispatch(&mut sim, &rules);
        assert_eq!(result.consequences.fire_events().len(), 1, "one shot");
        let mapped = |rx, ry| sim.fog.is_ground_unshrouded(americans, rx, ry);
        assert_eq!(mapped(3, 3), human, "the raised centre, human {human}");
        assert_eq!(mapped(5, 3), human, "radius 2, human {human}");
        assert!(!mapped(6, 3), "radius 2, human {human}");
        assert!(!mapped(10, 10), "not the ground under it, human {human}");
    }
}

/// `0x0041659E..0x004165AC`: a missile on the kamikaze tracker (`+0x6CA`,
/// set by its Push) that fires is removed (UnInit) after the shot.
#[test]
fn a_kamikaze_missile_is_removed_after_its_shot() {
    let (mut sim, rules) = fixture(&serde_json::json!({"burst": 1, "missile_spawn": true}));
    sim.kamikaze_push(
        1,
        None,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(sim.kamikaze.contains(1));
    let result = dispatch(&mut sim, &rules);
    assert_eq!(result.consequences.fire_events().len(), 1, "it fires");
    let missile = sim.substrate.entities.get(1).unwrap();
    assert!(!missile.lifecycle.object_alive(), "UnInit");
    assert!(sim.substrate.pending_delete.contains_object(&1));
}

/// Mission_Attack's strike: after the state-4 burst, Scatter_Objects on the
/// target's cell (Map[Target vt+0x48]) from the aircraft's Location, forced
/// and not no-kidding. A computer tank on the struck cell, north of the
/// aircraft, heads further north with a queued Move.
#[test]
fn an_aircraft_strike_scatters_the_target_cell_away_from_the_aircraft() {
    use crate::sim::mission::{MissionId, MissionType};
    let (mut sim, rules) = fixture(&serde_json::json!({
        "burst": 1,
        "extra_ini": "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=200\nSpeed=6\n\
                      SpeedType=Track\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    }));
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let russians = sim.interner.intern("Russians");
    let mut house = crate::sim::house_state::HouseState::new(russians, 0, None, false, 0, 10);
    house.current_iq = 5;
    sim.houses.insert(russians, house);
    let tank = sim
        .spawn_object("MTNK", "Russians", 10, 9, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .mission
        .set_current_for_test(MissionId::from_known(MissionType::Guard), 0);
    let result = dispatch(&mut sim, &rules);
    assert_eq!(result.consequences.fire_events().len(), 1, "one shot");
    let tank = sim.substrate.entities.get(tank).unwrap();
    assert!(
        matches!(
            tank.navigation.nav_com,
            Some(crate::sim::components::NavTargetRef::Cell { ry: 8, .. })
        ),
        "{:?}",
        tank.navigation.nav_com
    );
    assert_eq!(
        tank.mission.queued(),
        MissionId::from_known(MissionType::Move)
    );
}
