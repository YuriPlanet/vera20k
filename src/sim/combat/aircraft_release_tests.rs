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
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[AircraftTypes]\n0=ORCA\n[ORCA]\nStrength=150\nSpeed=8\nAmmo=2\nROT=5\n\
         Primary=Gun\n{}Fighter={}\nLocomotor={{4A582746-9839-11d1-B709-00A024DDAFD1}}\n\
         [Gun]\nDamage=10\nROF=20\nRange=20\nBurst={}\nOmniFire={}\nProjectile=Shell\nWarhead=WH\n\
         [EliteGun]\nDamage=10\nROF=37\nRange=20\nBurst={}\nProjectile=Shell\nWarhead=WH\n\
         [Shell]\nROT={}\nInviso={}\nAG=yes\nAA=yes\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        if elite { "ElitePrimary=EliteGun\n" } else { "" },
        input["fighter"].as_bool().unwrap_or(false),
        input["burst"].as_i64().unwrap_or(2),
        input["omni_fire"].as_bool().unwrap_or(false),
        input["elite_burst"].as_i64().unwrap_or(3),
        input["rot"].as_i64().unwrap_or(3),
        input["inviso"].as_bool().unwrap_or(true),
    )))
    .unwrap();
    let mut sim = Simulation::with_seed(0);
    let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
    entity.category = EntityCategory::Aircraft;
    entity.mission_leaf = MissionLeafState::aircraft_raw_for_test(0, 1, false);
    entity
        .mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: crate::sim::mission::MissionId::from_raw(1),
            suspended: crate::sim::mission::MissionId::NONE,
            queued: crate::sim::mission::MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
        });
    entity.aircraft_mission = Some(AircraftMission::Attack { sub_state: 4 });
    entity.aircraft_ammo = Some(AircraftAmmo::new(2));
    entity.aircraft_ammo.as_mut().unwrap().current = input["ammo"].as_i64().unwrap_or(2) as i32;
    entity.veterancy = (input["veterancy"].as_u64().unwrap_or(0) * 100) as u16;
    entity.body_facing = Some(FacingClass::new(0, 5));
    entity.barrel_facing = Some(FacingClass::new(0, 5));
    entity.attack_target = Some(AttackTarget::for_cell(10, 9));
    entity.locomotor = Some(LocomotorState::from_object_type(
        rules.object("ORCA").unwrap(),
        0,
    ));
    sim.substrate.entities.insert(entity);
    sim.substrate.next_stable_object_id = 2;
    sim.interner = test_interner();
    (sim, rules)
}

fn dispatch(sim: &mut Simulation, rules: &RuleSet) -> CombatTickResult {
    let requests = crate::sim::aircraft::tick_aircraft_missions(sim, rules, None);
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
        &[],
    )
}

#[test]
fn aircraft_release_control_matches_316_original_mission_suffixes() {
    // Native witness replaces FireAt with a NULL-return callback. Here actual
    // shared emission runs; only loop count, pending ammo and suffix are parity
    // assertions. Projectile math and GetROF are outside that native witness.
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_attack_release.json"
    ))
    .unwrap();
    let rows = corpus["releases"].as_array().unwrap();
    assert_eq!(rows.len(), 316);
    for row in rows {
        let (mut sim, rules) = fixture(&row["input"]);
        let result = dispatch(&mut sim, &rules);
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            entity.aircraft_ammo.as_ref().unwrap().release_pending(),
            true,
            "{row}"
        );
        assert_eq!(
            entity.aircraft_ammo.as_ref().unwrap().current,
            row["input"]["ammo"].as_i64().unwrap() as i32,
            "{row}"
        );
        let Some(AircraftMission::Attack { sub_state }) = entity.aircraft_mission else {
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
    sim.substrate.entities.get_mut(1).unwrap().aircraft_mission =
        Some(AircraftMission::Attack { sub_state: 3 });
    assert!(
        dispatch(&mut sim, &rules)
            .consequences
            .fire_events()
            .is_empty()
    );
    let attack = sim
        .substrate
        .entities
        .get_mut(1)
        .unwrap()
        .attack_target
        .as_mut()
        .unwrap();
    attack.cooldown_ticks = 4;
    assert!(
        dispatch(&mut sim, &rules)
            .consequences
            .fire_events()
            .is_empty()
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .attack_target
            .as_ref()
            .unwrap()
            .cooldown_ticks,
        3
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
    assert!(crate::sim::aircraft::tick_aircraft_missions(&mut sim, &rules, None).is_empty());
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
    crate::sim::aircraft::tick_aircraft_missions(&mut sim, &rules, None);
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
        crate::sim::aircraft::tick_aircraft_missions(world, &rules, None);
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
    sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
    assert_eq!(sim.fire_events.len(), 2);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.aircraft_ammo.as_ref().unwrap().release_pending());
    assert_eq!(entity.aircraft_ammo.as_ref().unwrap().current, 2);
    assert!(matches!(
        entity.aircraft_mission,
        Some(AircraftMission::Attack { sub_state: 1 })
    ));
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
