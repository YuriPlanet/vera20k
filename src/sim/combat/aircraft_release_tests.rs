use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::ammo::TechnoAmmo;
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
         Primary=Gun\n{}Fighter={}\nLocomotor={{4A582746-9839-11d1-B709-00A024DDAFD1}}\n{}{}\
         [Gun]\nDamage=10\nROF=20\nRange=20\nBurst={}\nOmniFire={}\nProjectile=Shell\nWarhead=WH\n{}{}{}\
         [EliteGun]\nDamage=10\nROF=37\nRange=20\nBurst={}\nProjectile=Shell\nWarhead=WH\n\
         [Shell]\nROT={}\nInviso={}\nAG=yes\nAA=yes\n{}\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n{}{}",
        if elite { "ElitePrimary=EliteGun\n" } else { "" },
        input["fighter"].as_bool().unwrap_or(false),
        if input["missile_spawn"].as_bool().unwrap_or(false) {
            "MissileSpawn=yes\n"
        } else {
            ""
        },
        input["aircraft_ini"].as_str().unwrap_or(""),
        input["burst"].as_i64().unwrap_or(2),
        input["omni_fire"].as_bool().unwrap_or(false),
        input["gun_ini"].as_str().unwrap_or(""),
        input["speed"]
            .as_i64()
            .map_or(String::new(), |speed| format!("Speed={speed}\n")),
        if input["suicide"].as_bool().unwrap_or(false) {
            "Suicide=yes\n"
        } else {
            ""
        },
        input["elite_burst"].as_i64().unwrap_or(3),
        input["rot"].as_i64().unwrap_or(3),
        input["inviso"].as_bool().unwrap_or(true),
        input["bullet_ini"].as_str().unwrap_or(""),
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
    entity.mission_leaf = MissionLeafState::aircraft_raw_for_test(0, 1);
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
    entity.install_ammo_for_test(Some(TechnoAmmo::new(2)));
    entity
        .ammo_mut()
        .unwrap()
        .set_current_for_test(input["ammo"].as_i64().unwrap_or(2) as i32);
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
        let mut cargo = crate::sim::passenger::PassengerCargo::new(8, 0.0);
        cargo.board_forced_chain(&[(2, 1.0)]);
        entity.set_passenger_role_for_test(crate::sim::passenger::PassengerRole::transport(cargo));
    }
    sim.substrate.entities.insert(entity);
    sim.set_next_stable_object_id_for_test(2);
    sim.interner = test_interner();
    if passenger {
        // Retail Clear ground and a playfield holding the aircraft's cell
        // (10, 10), where Paradrop places the passenger.
        sim.install_resolved_terrain_for_new_map(
            crate::map::resolved_terrain::test_retail_clear_grid(64, 64),
        );
        sim.set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
            base: 10,
            off_fc: 0,
            off_100: 0,
            off_104: 60,
            off_108: 60,
        }));
        let mut infantry = GameEntity::test_default(2, "E1", "Americans", 10, 10);
        infantry.set_owner_for_test(sim.interner.intern("Americans"));
        infantry.set_type_ref_for_test(sim.interner.intern("E1"));
        infantry.set_category_for_test(EntityCategory::Infantry);
        infantry.set_is_voxel_for_test(false);
        infantry.set_sub_cell(Some(2));
        infantry.set_passenger_role_for_test(
            crate::sim::passenger::PassengerRole::inside_for_test(1, false),
        );
        sim.substrate.entities.insert(infantry);
        sim.set_next_stable_object_id_for_test(3);
    }
    sim.set_logic_order_for_test(vec![1]);
    (sim, rules)
}

/// Dispatches every aircraft once; a strike fires inside its dispatch.
/// Returns the FireAt events the strikes emitted.
fn dispatch(sim: &mut Simulation, rules: &RuleSet) -> Vec<SimFireEvent> {
    let before = sim.fire_events.len();
    crate::sim::aircraft::tick_aircraft_missions(sim, rules);
    let mut events = sim.fire_events.take_for_test();
    let fired = events.split_off(before);
    sim.fire_events.emit_all(events);
    fired
}

#[test]
fn shared_fireat_refuses_positive_health_limbo_objects_before_emission() {
    // ObjectCtor5F3B37 ORs flag0x2; FireAt6FDD88..6FDDBE checks +81
    // on every Object. Supplied lifecycle controls isolate the common entry,
    // not an ordinary positive-health Limbo producer or aircraft sortie.
    for (kind, limbo, suicide, infantry) in [
        ("unit", true, false, false),
        ("building", true, false, false),
        ("terrain", true, false, false),
        ("unit", false, false, false),
        ("building", false, false, false),
        ("terrain", false, false, false),
        ("cell", false, false, false),
        ("terrain", true, true, false),
        ("terrain", true, false, true),
    ] {
        let (mut sim, rules) = fixture(&serde_json::json!({
            "suicide": suicide,
            "extra_ini": "[TerrainTypes]\n0=TREE01\n[TREE01]\nStrength=100\n\
                [VehicleTypes]\n0=TARGET\n[TARGET]\nStrength=100\n\
                [BuildingTypes]\n0=TARGETBUILD\n[TARGETBUILD]\nStrength=100\n"
        }));
        match kind {
            "unit" | "building" => {
                let (type_id, category) = if kind == "building" {
                    ("TARGETBUILD", EntityCategory::Structure)
                } else {
                    ("TARGET", EntityCategory::Unit)
                };
                let mut target = GameEntity::test_default(2, type_id, "Russians", 11, 10);
                target.set_type_ref_for_test(sim.interner.intern(type_id));
                target.set_category_for_test(category);
                target.lifecycle.set_in_limbo_for_test(false);
                sim.substrate.entities.insert(target);
            }
            "terrain" => {
                let type_ref = sim.interner.intern("TREE01");
                sim.terrain_objects.objects_mut_for_test().insert(
                    2,
                    crate::sim::terrain_object::TerrainObjectState::for_test(2, type_ref, 11, 10),
                );
            }
            "cell" => {}
            _ => unreachable!(),
        }
        let actor = sim.substrate.entities.get_mut(1).unwrap();
        actor.attack_target.set_for_test(Some(if kind == "cell" {
            TargetKind::Cell(11, 10)
        } else {
            TargetKind::Entity(2)
        }));
        if infantry {
            actor.set_category_for_test(EntityCategory::Infantry);
            actor.mission_leaf = MissionLeafState::infantry_raw_for_test(7, 0);
        }
        let shot = live_shot(&sim, &rules, 1)
            .unwrap()
            .1
            .expect("live target snapshot");
        if limbo {
            if let Some(target) = sim.substrate.entities.get_mut(2) {
                target.lifecycle.set_in_limbo_for_test(true);
                assert!(target.lifecycle.object_alive() && target.health.current() > 0);
            } else {
                let target = sim
                    .terrain_objects
                    .objects_mut_for_test()
                    .get_mut(&2)
                    .unwrap();
                target.set_lifecycle_for_test(
                    crate::sim::terrain_object::TerrainObjectLifecycle::Limbo,
                );
                assert!(target.object_alive() && target.health() > 0);
            }
        }
        let before = sim.state_hash();
        let rng = sim.rng_state();
        let actor = sim.substrate.entities.get(1).unwrap();
        let health = actor.health;
        let rearm = actor.rearm_timer;
        let mut out = CombatEmit::default();
        let bullet = emit_admitted_fire(
            &mut sim,
            &rules,
            shot,
            100,
            &mut out,
            None,
            FrameEffects::default(),
        );
        if limbo {
            assert_eq!(
                bullet, None,
                "{kind}/{suicide}/{infantry}: Object Limbo refuses FireAt"
            );
            assert!(sim.projectiles.is_empty());
            assert_eq!(sim.rng_state(), rng);
            let actor = sim.substrate.entities.get(1).unwrap();
            assert_eq!(
                actor.health, health,
                "Suicide must not reach the self receiver"
            );
            assert_eq!(actor.rearm_timer, rearm);
            if infantry {
                // Infantry51DF70 clears68D before the base refusal.
                assert_eq!(actor.mission_leaf.foot_firing_sequence_latch(), 0);
            } else {
                assert_eq!(sim.state_hash(), before, "{kind}: no other FireAt work");
            }
        } else {
            assert!(bullet.is_some(), "{kind}: live Object/Cell still launches");
        }
    }
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
        assert!(entity.ammo().unwrap().release_pending(), "{row}");
        assert_eq!(
            entity.ammo().unwrap().current(),
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
        assert_eq!(result.len(), calls, "{row}");
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
    assert!(dispatch(&mut sim, &rules).is_empty());
    let frame = sim.session.binary_frame() as i32;
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .rearm_timer
        .set_for_test(crate::sim::timer::CdTimer::started(frame, 4));
    assert!(dispatch(&mut sim, &rules).is_empty());
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
            .ammo()
            .unwrap()
            .release_pending()
    );
}

#[test]
fn aircraft_release_uses_raw_burst_above_byte_width_and_one_ammo_charge() {
    let (mut sim, rules) = fixture(&serde_json::json!({"burst":257,"fighter":true}));
    let result = dispatch(&mut sim, &rules);
    assert_eq!(result.len(), 257);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.ammo().unwrap().current(), 2);
    assert_eq!(entity.weapon_burst.index(), 0);
    // The Mission return owns cadence, independent of the final rearm jitter.
    sim.session.set_binary_frame_for_test(19);
    assert!(crate::sim::aircraft::tick_aircraft_missions(&mut sim, &rules).is_empty());
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .ammo()
            .unwrap()
            .current(),
        2
    );
    sim.session.set_binary_frame_for_test(20);
    crate::sim::aircraft::tick_aircraft_missions(&mut sim, &rules);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.ammo().unwrap().current(), 1);
    assert!(!entity.ammo().unwrap().release_pending());
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
    assert_eq!(dispatch(&mut sim, &rules).len(), 3);
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
        world.session.set_binary_frame_for_test(20);
        crate::sim::aircraft::tick_aircraft_missions(world, &rules);
        let entity = world.substrate.entities.get(1).unwrap();
        assert_eq!(entity.weapon_burst.index(), 1);
        assert_eq!(entity.ammo().unwrap().current(), 1);
        assert!(!entity.ammo().unwrap().release_pending());
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
    assert!(entity.ammo().unwrap().release_pending());
    assert_eq!(entity.ammo().unwrap().current(), 2);
    assert_eq!(crate::sim::aircraft::attack_state(entity), Some(1));
}

/// A strike fires in the aircraft's own Logic slot: `MissionClass::AI`
/// (`0x005B3060`) runs Mission_Attack inside `FootClass::AI @ 0x004DA530`,
/// before the next LogicVector object's AI. A Unit later in the vector fires
/// after it in the same pass. VERA used to run every strike after the pass.
#[test]
fn a_strike_fires_before_a_later_unit_in_the_same_pass() {
    let (mut sim, rules) = fixture(&serde_json::json!({
        "burst": 1,
        "fighter": true,
        "extra_ini": "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=300\nSpeed=4\nPrimary=Gun\n",
    }));
    let mut tank = GameEntity::test_default(3, "TANK", "Americans", 12, 10);
    tank.set_owner_for_test(sim.interner.intern("Americans"));
    tank.set_type_ref_for_test(sim.interner.intern("TANK"));
    tank.set_category_for_test(EntityCategory::Unit);
    tank.body_facing = FacingClass::new(0, 5);
    tank.locomotor = Some(LocomotorState::from_object_type(
        rules.object("TANK").unwrap(),
        0,
    ));
    tank.attack_target
        .set_for_test(Some(TargetKind::Cell(12, 9)));
    sim.substrate.entities.insert(tank);
    sim.set_next_stable_object_id_for_test(4);
    sim.set_logic_order_for_test(vec![1, 3]);
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    let shooters: Vec<u64> = sim
        .fire_events
        .iter()
        .map(|event| event.attacker_id)
        .collect();
    assert_eq!(shooters, vec![1, 3]);
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
                result.len(),
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
        sim.session.set_binary_frame_for_test(frame);
        let fired = dispatch(&mut sim, &rules).len();
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
    let ammo = entity.ammo().unwrap();
    assert!(ammo.release_pending(), "state 3 pays it");
    assert_eq!(ammo.current(), 1);
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
    assert!(dispatch(&mut sim, &rules).is_empty());
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(crate::sim::aircraft::attack_state(entity), Some(5));
    assert_eq!(entity.mission.dispatch_timer().delay(), 1);

    let before = sim.scenario_rng.clone();
    sim.session.set_binary_frame_for_test(1);
    assert!(dispatch(&mut sim, &rules).is_empty());
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
        sim.session.set_binary_frame_for_test(frame);
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
        .velocity()
        .native()
        .map(|axis| f64::from_bits(axis.bits()));
    let speed = (x * x + y * y + z * z).sqrt();
    assert!((speed - 76.0).abs() < 0.5, "at 76: ({x}, {y}, {z})");
    assert!(
        y < -75.0 && x.abs() < 1.0,
        "north, at the target: ({x}, {y}, {z})"
    );
}

/// Phobos `AircraftClass_FireAt_Vertical` (`0x415F25`,
/// `Misc/Hooks.BugFixes.cpp:2423`): a `Vertical=` bullet whose
/// `Vertical.AircraftFix` (else the `[General]` key, default yes) is on keeps
/// FireAt's launch velocity instead of the ROT 1 course; the bullet type's
/// `no` and the global's `no` each re-enable the course.
#[test]
fn a_vertical_aircraft_bomb_keeps_fire_at_velocity() {
    let speed = |bullet_ini: &str, extra_ini: &str| {
        let (mut sim, rules) = fixture(&serde_json::json!({
            "burst": 1, "rot": 1, "inviso": false, "ammo": 1, "speed": 30,
            "bullet_ini": bullet_ini,
            "extra_ini": extra_ini
        }));
        let bomb = first_bullet(&mut sim, &rules);
        let [x, y, z] = bomb
            .velocity()
            .native()
            .map(|axis| f64::from_bits(axis.bits()));
        (x * x + y * y + z * z).sqrt()
    };
    let fixed = speed("Vertical=yes\n", "");
    assert!(
        (fixed - 1.0).abs() < 0.1,
        "the default-global fix keeps FireAt's unit launch: {fixed}"
    );
    let type_off = speed("Vertical=yes\nVertical.AircraftFix=no\n", "");
    assert!(
        (type_off - 76.0).abs() < 0.5,
        "the bullet type's no applies the course: {type_off}"
    );
    let global_off = speed("Vertical=yes\n", "[General]\nVertical.AircraftFix=no\n");
    assert!(
        (global_off - 76.0).abs() < 0.5,
        "the unset type key takes the global no: {global_off}"
    );
}

/// `0x00415EEE..0x00415F05`: an aircraft carrying a passenger drops it/// (`Drop_Payload`) where it would fire, and its strike carries on as after
/// a shot. VERA used to hold such an aircraft in state 4 with no shot.
#[test]
fn an_aircraft_carrying_a_passenger_drops_it_instead_of_firing() {
    let (mut sim, rules) = fixture(&serde_json::json!({"burst": 1, "passenger": true}));
    let result = dispatch(&mut sim, &rules);
    assert!(result.is_empty(), "no shot");
    assert!(sim.projectiles.is_empty(), "no bullet");
    let aircraft = sim.substrate.entities.get(1).unwrap();
    assert!(aircraft.passenger_role().cargo().unwrap().is_empty());
    assert_eq!(crate::sim::aircraft::attack_state(aircraft), Some(5));
    let dropped = sim.substrate.entities.get(2).unwrap();
    assert!(!dropped.passenger_role().is_inside_transport(), "dropped");
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
        aircraft.passenger_role().cargo().unwrap().is_empty(),
        "dropped"
    );
    assert_eq!(aircraft.ammo().unwrap().current(), 0);
    assert_eq!(crate::sim::aircraft::attack_state(aircraft), Some(10));
}

/// Phobos `AircraftClass_Fire_KickOutPassengers` (`0x415EEE`,
/// `Ext/Aircraft/Hooks.cpp:455-474`): the firing weapon's
/// `KickOutPassengers`, else `[General] AircraftWeapon.KickOutPassengers=`,
/// keeps the stock drop when yes; no skips it (SkipKickOutPassengers
/// `0x415F08`) and lets the shot fire.
#[test]
fn phobos_kick_out_passengers_gates_the_fire_drop() {
    let run = |input: Value| {
        let (mut sim, rules) = fixture(&input);
        let fired = dispatch(&mut sim, &rules).len();
        let aircraft = sim.substrate.entities.get(1).unwrap();
        (aircraft.passenger_role.cargo().unwrap().is_empty(), fired)
    };
    let (dropped, fired) = run(serde_json::json!({"burst": 1, "passenger": true}));
    assert_eq!((dropped, fired), (true, 0), "the default keeps the drop");

    let weapon_off = serde_json::json!({
        "burst": 1,
        "passenger": true,
        "gun_ini": "KickOutPassengers=no\n",
    });
    let (dropped, fired) = run(weapon_off);
    assert_eq!((dropped, fired), (false, 1), "the weapon key no fires");

    let global_off = serde_json::json!({
        "burst": 1,
        "passenger": true,
        "extra_ini": "[General]\nAircraftWeapon.KickOutPassengers=no\n",
    });
    let (dropped, fired) = run(global_off);
    assert_eq!((dropped, fired), (false, 1), "the global no fires");

    let weapon_on = serde_json::json!({
        "burst": 1,
        "passenger": true,
        "gun_ini": "KickOutPassengers=yes\n",
        "extra_ini": "[General]\nAircraftWeapon.KickOutPassengers=no\n",
    });
    let (dropped, fired) = run(weapon_on);
    assert_eq!((dropped, fired), (true, 0), "the weapon key yes drops");
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
        sim.fog.set_width_for_test(64);
        sim.fog.set_height_for_test(64);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(1500));
        let americans = sim.interner.intern("Americans");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                americans, 0, None, human, 0, 10,
            ));
        let result = dispatch(&mut sim, &rules);
        assert_eq!(result.len(), 1, "one shot");
        let mapped = |rx, ry| sim.fog.is_ground_unshrouded(americans, rx, ry);
        assert_eq!(mapped(3, 3), human, "the raised centre, human {human}");
        assert_eq!(mapped(5, 3), human, "radius 2, human {human}");
        assert!(!mapped(6, 3), "radius 2, human {human}");
        assert!(!mapped(10, 10), "not the ground under it, human {human}");
    }
}

/// In a campaign IsHumanPlayer (`0x0050B6F0`) admits the player's
/// PlayerControl ally (ALL01's Alliance flies Harriers), so its aircraft asks
/// the player's map, and RevealArea's house gate (`0x00567AB0..0x00567B12`)
/// maps the cells there under `AllyReveal=`.
#[test]
fn a_campaign_ally_aircraft_shooting_over_shroud_maps_the_players_cells() {
    let (mut sim, mut rules) = fixture(&serde_json::json!({"burst": 1}));
    rules.general.attacking_aircraft_sight_range = 2;
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
        64,
        64,
        crate::map::resolved_terrain::test_flat_cell,
    ));
    sim.fog.set_width_for_test(64);
    sim.fog.set_height_for_test(64);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(1500));
    let americans = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            americans, 0, None, true, 0, 10,
        ));
    sim.session.set_current_house_for_test(Some(americans));
    let alliance = sim.interner.intern("Alliance");
    let mut ally = crate::sim::house_state::HouseState::new(alliance, 0, None, false, 0, 10);
    ally.set_player_control_for_test(true);
    sim.houses.register(ally);
    sim.house_alliances_mut_for_test()
        .insert("ALLIANCE".into(), ["AMERICANS".to_string()].into());
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_owner_for_test(alliance);
    let result = dispatch(&mut sim, &rules);
    assert_eq!(result.len(), 1, "one shot");
    assert!(sim.fog.is_ground_unshrouded(americans, 3, 3));
    assert!(sim.fog.is_ground_unshrouded(americans, 5, 3), "radius 2");
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
    assert_eq!(result.len(), 1, "it fires");
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
    house.set_current_iq_for_test(5);
    sim.houses.register(house);
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
    assert_eq!(result.len(), 1, "one shot");
    let tank = sim.substrate.entities.get(tank).unwrap();
    assert!(
        matches!(
            tank.navigation.nav_com(),
            Some(crate::sim::components::NavTargetRef::Cell { ry: 8, .. })
        ),
        "{:?}",
        tank.navigation.nav_com()
    );
    assert_eq!(
        tank.mission.queued(),
        MissionId::from_known(MissionType::Move)
    );
}

/// Phobos `FiringForceScatter=` (`Ext/AircraftType/Body.cpp:36`; hooks
/// `0x41847E`/`0x4186DD`/`0x41882C`/`0x41893B`/`0x418A4A`/`0x418B46` at
/// `Ext/Aircraft/Hooks.cpp:341-355`) suppresses the forced target-cell
/// Scatter. Same setup as the test above, only the type key differs: the
/// computer tank on the struck cell keeps its Guard and no Move is queued.
#[test]
fn firing_force_scatter_false_leaves_the_struck_cell_alone() {
    use crate::sim::mission::{MissionId, MissionType};
    let (mut sim, rules) = fixture(&serde_json::json!({
        "burst": 1,
        "aircraft_ini": "FiringForceScatter=no\n",
        "extra_ini": "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=200\nSpeed=6\n\
                      SpeedType=Track\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    }));
    assert_eq!(
        rules.object("ORCA").unwrap().phobos_firing_force_scatter,
        Some(false)
    );
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let russians = sim.interner.intern("Russians");
    let mut house = crate::sim::house_state::HouseState::new(russians, 0, None, false, 0, 10);
    house.set_current_iq_for_test(5);
    sim.houses.register(house);
    let tank = sim
        .spawn_object("MTNK", "Russians", 10, 9, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .mission
        .set_current_for_test(MissionId::from_known(MissionType::Guard), 0);
    assert_eq!(
        dispatch(&mut sim, &rules).len(),
        1,
        "the release still fires"
    );
    let tank = sim.substrate.entities.get(tank).unwrap();
    assert!(
        tank.navigation.nav_com().is_none(),
        "{:?}",
        tank.navigation.nav_com()
    );
    assert_eq!(
        tank.mission.queued(),
        MissionId::NONE,
        "no Scatter-queued Move"
    );
}

/// Phobos `Strafing.Shots` (`Ext/WeaponType/Body.cpp:109`) caps the round
/// (`Ext/Aircraft/Hooks.cpp:37-51`) and the ending shot takes
/// `Strafing.EndDelay` (`:221`) or, unset, `(Range + 4 * LeptonsPerCell) /
/// Speed`. Production dispatch: one shot per visit, the release first.
#[test]
fn strafing_shots_cap_the_round_and_end_delay_overrides_the_derived_value() {
    let cap = serde_json::json!({
        "burst": 1, "rot": 1, "inviso": false,
        "gun_ini": "Strafing.Shots=3\n",
    });
    let (mut sim, rules) = fixture(&cap);
    assert_eq!(dispatch(&mut sim, &rules).len(), 1, "the release");
    assert_eq!(
        crate::sim::aircraft::attack_state(sim.substrate.entities.get(1).unwrap()),
        Some(6)
    );
    sim.session.set_binary_frame_for_test(100);
    assert_eq!(dispatch(&mut sim, &rules).len(), 1, "state 6's shot");
    sim.session.set_binary_frame_for_test(200);
    assert_eq!(dispatch(&mut sim, &rules).len(), 1, "state 7's shot");
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        crate::sim::aircraft::attack_state(entity),
        Some(3),
        "the third bomb reaches Strafing.Shots and the round ends"
    );
    assert_eq!(
        entity
            .mission_leaf
            .aircraft_strafe_bombs_dropped_this_round(),
        3
    );
    let weapon = rules.weapon("Gun").unwrap();
    let speed = crate::util::fixed_math::ra2_speed_to_leptons_per_frame(
        rules.object("ORCA").unwrap().speed,
    );
    let derived = (weapon.range_leptons + 4 * 256) / speed;
    assert_eq!(entity.mission.dispatch_timer().delay(), derived);

    // `Strafing.EndDelay=17` replaces the derived value on the same arm.
    let (mut sim, rules) = fixture(&serde_json::json!({
        "burst": 1, "rot": 1, "inviso": false,
        "gun_ini": "Strafing.Shots=3\nStrafing.EndDelay=17\n",
    }));
    for frame in [0, 100, 200] {
        sim.session.set_binary_frame_for_test(frame);
        dispatch(&mut sim, &rules);
    }
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(crate::sim::aircraft::attack_state(entity), Some(3));
    assert_eq!(entity.mission.dispatch_timer().delay(), 17, "EndDelay");
}

/// Phobos `Strafing.UseAmmoPerShot` (`Ext/Aircraft/Body.cpp:36-45`): one Ammo
/// per shot, `ShouldLoseAmmo` (`+0x6C8`) cleared, and Ammo 0 clears the
/// Target and Destination; `GetDelay` then ends the round (`Hooks.cpp:218`).
#[test]
fn strafing_ammo_per_shot_debits_each_shot_and_ends_the_round_at_zero() {
    let (mut sim, rules) = fixture(&serde_json::json!({
        "burst": 1, "rot": 1, "inviso": false, "ammo": 2,
        "gun_ini": "Strafing.UseAmmoPerShot=yes\n",
    }));
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    assert_eq!(dispatch(&mut sim, &rules).len(), 1);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.ammo().unwrap().current(), 1);
    assert!(
        !entity.ammo().unwrap().release_pending(),
        "ShouldLoseAmmo cleared: no round debit later"
    );
    assert_eq!(crate::sim::aircraft::attack_state(entity), Some(6));

    sim.session.set_binary_frame_for_test(100);
    assert_eq!(dispatch(&mut sim, &rules).len(), 1);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.ammo().unwrap().current(), 0);
    assert_eq!(entity.attack_target.target(), None, "SetTarget(nullptr)");
    assert_eq!(
        crate::sim::aircraft::attack_state(entity),
        Some(3),
        "UseAmmoPerShot with Ammo 0 ends the round"
    );
}

/// Phobos `Strafing.TargetCell` (`Ext/Aircraft/Hooks.cpp:233-235`) stores the
/// Target's cell on the state-4 suffix, and states 6..9 fire at it
/// (`:245-266`). `Strafing.SimulateBurst` writes `CurrentBurstIndex =
/// bombDropCount % 2 == 0` across the single-shot passes
/// (`Ext/Aircraft/Body.cpp:26-27`); a shot FireAt refuses leaves that write
/// in the retained index.
#[test]
fn strafing_target_cell_and_simulate_burst_change_the_shots() {
    let (mut sim, rules) = fixture(&serde_json::json!({
        "burst": 1, "rot": 1, "inviso": false,
        "gun_ini": "Strafing.TargetCell=yes\n",
        "extra_ini": "[VehicleTypes]\n0=TARGET\n[TARGET]\nStrength=100\n",
    }));
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let mut target = GameEntity::test_default(2, "TARGET", "Russians", 10, 9);
    target.set_type_ref_for_test(sim.interner.intern("TARGET"));
    target.set_category_for_test(EntityCategory::Unit);
    target.lifecycle.set_in_limbo_for_test(false);
    sim.substrate.entities.insert(target);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(2)));

    let release = dispatch(&mut sim, &rules);
    assert_eq!(release.len(), 1);
    assert_eq!(
        release[0].target,
        TargetKind::Entity(2),
        "the release still aims at the Target"
    );
    sim.session.set_binary_frame_for_test(100);
    let stored = dispatch(&mut sim, &rules);
    assert_eq!(stored.len(), 1, "state 6's shot");
    assert_eq!(
        stored[0].target,
        TargetKind::Cell(10, 9),
        "the round fires at the Target's stored cell"
    );
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::cell(10, 9)),
        "SetDestination(target cell, 1)"
    );

    // SimulateBurst: the first pass reads bomb count 0 and stores index 1.
    // FireAt refuses a Limbo Object before launch, so the write stays visible.
    let (mut sim, rules) = fixture(&serde_json::json!({
        "burst": 1, "rot": 1, "inviso": false,
        "gun_ini": "Strafing.SimulateBurst=yes\n",
        "extra_ini": "[TerrainTypes]\n0=TREE01\n[TREE01]\nStrength=100\n",
    }));
    let type_ref = sim.interner.intern("TREE01");
    sim.terrain_objects.objects_mut_for_test().insert(
        2,
        crate::sim::terrain_object::TerrainObjectState::for_test(2, type_ref, 10, 9),
    );
    let tree = sim
        .terrain_objects
        .objects_mut_for_test()
        .get_mut(&2)
        .unwrap();
    tree.set_lifecycle_for_test(crate::sim::terrain_object::TerrainObjectLifecycle::Limbo);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(2)));
    assert!(
        dispatch(&mut sim, &rules).is_empty(),
        "Limbo refuses FireAt"
    );
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().weapon_burst.index(),
        1,
        "bombDropCount 0 stores CurrentBurstIndex 1"
    );
}

/// Phobos `AircraftClass_Mission_Attack_StrafeShots`'s re-entry
/// (`Ext/Aircraft/Hooks.cpp:39-48`): above five shots, FireAtTarget3_Strafe
/// (7) goes back to FireAtTarget2_Strafe (6) while
/// `Strafing.Shots - 3 - counter > 0`, so the round fires the extra bombs
/// from state 6. Production dispatch: one shot per visit.
#[test]
fn strafing_shots_above_five_reenter_state_six_until_the_bombs_run_out() {
    let (mut sim, rules) = fixture(&serde_json::json!({
        "burst": 1, "rot": 1, "inviso": false,
        "gun_ini": "Strafing.Shots=7\n",
    }));
    let mut bombs = 0;
    let mut visits = Vec::new();
    for frame in [0, 100, 200, 300, 400, 500, 600] {
        sim.session.set_binary_frame_for_test(frame);
        bombs += dispatch(&mut sim, &rules).len();
        let entity = sim.substrate.entities.get(1).unwrap();
        visits.push((
            crate::sim::aircraft::attack_state(entity),
            entity
                .mission_leaf
                .aircraft_strafe_bombs_dropped_this_round(),
        ));
    }
    assert_eq!(bombs, 7, "release plus six bombs");
    assert_eq!(
        visits,
        [
            (Some(6), 1),
            (Some(7), 2),
            (Some(7), 3),
            (Some(7), 4),
            (Some(8), 5),
            (Some(9), 6),
            (Some(3), 7),
        ],
        "two state-7 visits re-enter state 6, then 7, 8 and 9 end the run"
    );
    let weapon = rules.weapon("Gun").unwrap();
    let speed = crate::util::fixed_math::ra2_speed_to_leptons_per_frame(
        rules.object("ORCA").unwrap().speed,
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .mission
            .dispatch_timer()
            .delay(),
        (weapon.range_leptons + 4 * 256) / speed,
        "the last shot takes the derived end delay"
    );
}
