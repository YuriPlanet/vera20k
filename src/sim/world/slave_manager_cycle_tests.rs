//! A Yuri slave refinery's harvest cycle through the production frame
//! (`advance_tick`) on the oracle replay's scene
//! (`slave_manager_oracle_tests::row_scene`): the manager leaving state 0,
//! DeploySlaves, the slaves walking to ore, digging one level per
//! `HarvestRate`, carrying the load home, paying it and going back out. The
//! per-step behaviour is pinned row by row against gamemd in
//! `slave_manager_oracle_tests`; these pin how the steps compose over frames.

use super::harvest_field_oracle_tests::registry;
use super::slave_manager_oracle_tests::{SlaveScene, row_scene};
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::NavTargetRef;
use crate::sim::mission::MissionType;
use crate::sim::ore_growth::OreGrowthConfig;
use crate::sim::slave_manager::{ManagerState, SlaveState};
use crate::sim::world::SimSoundEvent;

fn frame(s: &mut SlaveScene) {
    let grid = s.scene.sim.path_grid_snapshot();
    let commands = s.scene.sim.take_due_commands();
    s.scene.sim.advance_tick(
        &commands,
        Some(&s.scene.rules),
        grid.as_deref(),
        Some(registry()),
        67,
    );
}

/// A built refinery (manager state 0) holding `count` slaves inside, with a
/// density-5 ore patch east of it and no growth.
fn refinery_with_slaves(count: usize) -> SlaveScene {
    let slaves: Vec<_> = (0..count)
        .map(|_| serde_json::json!({"state": 0, "cell": [13, 13], "limbo": true}))
        .collect();
    let mut s = row_scene(&serde_json::json!({
        "name": "cycle",
        "manager_state": 0,
        "nodes": slaves,
        "ore": [
            [17, 13, 0, 0, 5], [18, 13, 0, 0, 5], [17, 14, 0, 0, 5],
            [18, 14, 0, 0, 5], [17, 12, 0, 0, 5], [18, 12, 0, 0, 5],
        ],
    }));
    s.scene.sim.ore_growth_config = OreGrowthConfig::disabled();
    s
}

fn node_states(s: &SlaveScene) -> Vec<SlaveState> {
    s.scene
        .sim
        .substrate
        .entities
        .get(s.master)
        .unwrap()
        .slave_manager
        .as_ref()
        .unwrap()
        .nodes()
        .iter()
        .map(|node| node.state())
        .collect()
}

fn manager_state(s: &SlaveScene) -> ManagerState {
    s.scene
        .sim
        .substrate
        .entities
        .get(s.master)
        .unwrap()
        .slave_manager
        .as_ref()
        .unwrap()
        .state()
}

fn credits(s: &SlaveScene) -> i32 {
    let owner = s.scene.sim.interner.get("Americans").unwrap();
    s.scene.sim.houses[&owner].economy().credits()
}

/// The first visit (10 frames) moves the built refinery to state 5; the next
/// lets every slave out onto the drop cell's spots and scatters them; the
/// third sends each to ore. Each digs one level per 150 frames, walks its
/// four levels home, pays them (100 credits of ore) inside the refinery,
/// reloads for 25 frames and goes out again.
#[test]
fn slave_refinery_deploys_digs_carries_home_pays_and_goes_out_again() {
    let mut s = refinery_with_slaves(3);
    let slaves: Vec<u64> = s.slaves.values().copied().collect();
    for _ in 0..11 {
        frame(&mut s);
    }
    assert_eq!(manager_state(&s), ManagerState::Working);
    for _ in 0..10 {
        frame(&mut s);
    }
    assert_eq!(
        node_states(&s),
        vec![SlaveState::Scanning; 3],
        "all three out"
    );
    for &slave in &slaves {
        let entity = s.scene.sim.substrate.entities.get(slave).unwrap();
        assert!(!entity.lifecycle.in_limbo());
        assert!(
            entity.navigation.nav_com().is_some(),
            "scattered off the drop cell"
        );
    }

    let mut dug = false;
    let mut shovel_shown = false;
    let mut carry_shown = false;
    let mut empty_walk_after_payment = false;
    let mut paid_at = None;
    let start = credits(&s);
    for n in 0..3000 {
        frame(&mut s);
        for &slave in &slaves {
            let entity = s.scene.sim.substrate.entities.get(slave).unwrap();
            if let Some((doing, _)) = entity.infantry_sprite_pose() {
                if doing == 39 {
                    assert_eq!(entity.slave.cargo().len(), 4, "Carry needs a full load");
                    carry_shown = true;
                    assert_eq!(
                        s.scene
                            .rules
                            .animation_sequence("SLAV")
                            .unwrap()
                            .infantry_action(39)
                            .unwrap()
                            .start_frame,
                        228
                    );
                }
                empty_walk_after_payment |=
                    paid_at.is_some() && doing == 3 && entity.slave.cargo().is_empty();
            }
            dug |= !entity.slave.cargo().is_empty();
            shovel_shown |= entity
                .infantry_sprite_pose()
                .is_some_and(|(doing, _)| doing == 38);
        }
        if paid_at.is_none() && credits(&s) > start {
            paid_at = Some(n);
        }
        if paid_at.is_some_and(|paid| n > paid + 60) {
            break;
        }
    }
    assert!(dug, "the slaves reach the ore and dig");
    assert!(shovel_shown, "a digging slave shows its Shovel sequence");
    assert!(carry_shown, "a full returning slave shows the Carry bank");
    assert!(
        empty_walk_after_payment,
        "after deposit an empty slave walks normally"
    );
    let paid_at = paid_at.expect("a full slave carries its load home and pays");
    assert_eq!(
        (credits(&s) - start) % 100,
        0,
        "each visit pays four levels of ore"
    );
    // A slave that paid reloaded inside and went out again.
    assert!(
        node_states(&s)
            .iter()
            .any(|state| matches!(state, SlaveState::Scanning | SlaveState::Moving)),
        "back out after the reload (paid at {paid_at}): {:?}",
        node_states(&s)
    );
}

fn manager_state_of(s: &SlaveScene, holder: u64) -> ManagerState {
    s.scene
        .sim
        .substrate
        .entities
        .get(holder)
        .unwrap()
        .slave_manager
        .as_ref()
        .unwrap()
        .state()
}

/// Hits on `target` from `attacker` through the receiver until it dies
/// (`MaxDamage` caps each one).
fn kill(s: &mut SlaveScene, target: u64, attacker: Option<u64>) {
    let warhead = s.scene.sim.interner.intern("KILLWH");
    let house = attacker
        .and_then(|id| s.scene.sim.substrate.entities.get(id))
        .map(|entity| entity.owner());
    for _ in 0..10 {
        if !s
            .scene
            .sim
            .substrate
            .entities
            .get(target)
            .is_some_and(|entity| entity.health.current() > 0)
        {
            return;
        }
        let hit = EntityDamageEvent::direct_receiver(
            target,
            100_000,
            0,
            attacker.unwrap_or(RAD_NO_ATTACKER),
            house,
            warhead,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        s.scene.sim.commit_noncombat_aoe_hits(
            &s.scene.rules,
            Some(registry()),
            &[hit],
            crate::sim::world::FrameEffects::default(),
        );
    }
    panic!("{target} survived ten hits");
}

/// The death arm frees the refinery's slaves (`0x006B0AE0`): the two outside
/// pass to the killer's house, reset to Guard and cheer, and `SlavesFreeSound`
/// plays at the first; the one reloading inside dies with the refinery. Once
/// the cheer has played, an order the new owner gave meanwhile commences.
#[test]
fn a_destroyed_refinery_frees_its_slaves_to_the_killer() {
    let mut s = row_scene(&serde_json::json!({
        "name": "free",
        "nodes": [
            {"state": 3, "cell": [17, 12], "mission": "harvest"},
            {"state": 1, "cell": [16, 14]},
            {"state": 5, "cell": [13, 13], "limbo": true, "timer": [-5, 25]},
        ],
        "ore": [[17, 12, 0, 0, 5]],
    }));
    s.scene.rules.general.slaves_free_sound = Some("SlavesFreed".to_string());
    let russians = s.scene.sim.interner.intern("Russians");
    s.scene
        .sim
        .houses
        .register(crate::sim::house_state::HouseState::new(
            russians, 1, None, true, 0, 10,
        ));
    let killer = s
        .scene
        .sim
        .spawn_object("MTNK", "Russians", 10, 20, 0, &s.scene.rules)
        .expect("killer");
    s.scene.sim.sound_events.clear_for_test();
    let master = s.master;
    kill(&mut s, master, Some(killer));

    let sim = &s.scene.sim;
    assert!(
        sim.substrate
            .entities
            .get(s.master)
            .unwrap()
            .slave_manager
            .is_none()
    );
    for index in [0, 1] {
        let slave = sim.substrate.entities.get(s.slaves[&index]).unwrap();
        assert_eq!(slave.owner(), russians, "slave {index} joins the killer");
        assert_eq!(slave.slave.owner(), None);
        assert_eq!(slave.mission.current().known(), Some(MissionType::Guard));
        assert_eq!(
            slave.mission_leaf.as_infantry().unwrap().doing(),
            32,
            "it cheers"
        );
    }
    let inside = sim.substrate.entities.get(s.slaves[&2]).unwrap();
    assert!(
        !inside.lifecycle.object_alive(),
        "the slave inside dies with it"
    );
    // The first freed is the last node's live slave: node 1.
    let freed = sim.substrate.entities.get(s.slaves[&1]).unwrap();
    assert!(sim.sound_events.iter().any(|event| matches!(
        event,
        SimSoundEvent::VocAt { sound_id, rx, ry, .. }
            if sound_id == "SlavesFreed" && (*rx, *ry) == (freed.position.rx(), freed.position.ry())
    )));

    // The Cheer cannot be interrupted: a Move order given during it walks the
    // slave off at once, but its mission waits in the queue
    // (Ready_To_Commence). The Cheer's end, `InfantryClass::DoType_Sequencer`'s
    // default arm, forces Ready on a standing slave or Walk on a moving one;
    // the queued Move then commences.
    let ordered = s.slaves[&1];
    let tick = s.scene.sim.session.tick();
    s.scene.sim.queue_command(CommandEnvelope::new(
        russians,
        tick + 1,
        Command::Move {
            entity_id: ordered,
            target_rx: 18,
            target_ry: 17,
        },
    ));
    let doing = |s: &SlaveScene, id: u64| {
        s.scene
            .sim
            .substrate
            .entities
            .get(id)
            .unwrap()
            .mission_leaf
            .as_infantry()
            .unwrap()
            .doing()
    };
    let mission = |s: &SlaveScene, id: u64| {
        s.scene
            .sim
            .substrate
            .entities
            .get(id)
            .unwrap()
            .mission
            .current()
            .known()
    };
    let mut cheered = 0;
    while doing(&s, s.slaves[&0]) == 32 {
        assert!(cheered < 60, "the cheer ends");
        assert_eq!(
            mission(&s, ordered),
            Some(MissionType::Guard),
            "the Move waits for the cheer"
        );
        frame(&mut s);
        cheered += 1;
    }
    assert!(cheered >= 20, "the cheer plays its 8 frames: {cheered}");
    assert_eq!(
        doing(&s, s.slaves[&0]),
        0,
        "a standing slave returns to Ready"
    );
    // Native520D38/520D71..79 forces Walk3 after a moving Cheer completes;
    // original jumpjet_infantry_actions row522 pins that common default arm.
    // This cycle exercises the production SLAV consumer, not native FreeSlaves.
    assert_eq!(doing(&s, ordered), 3, "a moving slave resumes Walk");
    for _ in 0..3 {
        frame(&mut s);
    }
    assert_eq!(
        mission(&s, ordered),
        Some(MissionType::Move),
        "the queued Move commences"
    );
}

/// Deploying a Slave Miner (`UnitClass::Deploy`, `deploy_mcv`) and undeploying
/// its refinery (`undeploy_building` to its conversion in
/// `visit_building_down`) each hand the manager over (SetOwner `0x006AF580`):
/// the slaves keep their nodes and follow it, and the manager the new
/// object's constructor built frees its own fresh slaves (UnInit in limbo).
/// Each constructor draws its TechnoClass word and one per slave it builds.
#[test]
fn deploy_and_undeploy_hand_the_slave_manager_over() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::rng::SimRng;
    use crate::sim::world::Simulation;
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[InfantryTypes]\n1=SLAV\n[VehicleTypes]\n1=SMIN\n[BuildingTypes]\n1=YAREFN\n\
         [SLAV]\nStrength=125\nSpeed=3\nSlaved=yes\nStorage=4\nHarvestRate=150\n\
         [SMIN]\nStrength=2000\nSpeed=3\nEnslaves=SLAV\nSlavesNumber=5\nDeploysInto=YAREFN\n\
         ResourceGatherer=yes\nResourceDestination=yes\n\
         [YAREFN]\nStrength=2000\nEnslaves=SLAV\nSlavesNumber=5\nUndeploysInto=SMIN\n\
         DeployFacing=0\n[Clear]\nBuildable=yes\n",
        ),
        &IniFile::from_str("[YAREFN]\nFoundation=3x3\n"),
    )
    .expect("slave miner rules");
    // A Buildup SHP, without which the refinery cannot undeploy
    // (`Sell_Back @ 0x00447110`).
    let mut rules = rules;
    rules.set_buildup_control_for_test("YAREFN", [0, 17, 3]);
    let seed = 0x51A7_E001;
    let mut sim = Simulation::with_seed(seed);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let mut expected = SimRng::new(seed);
    let constructed = |sim: &Simulation, expected: &mut SimRng| {
        for _ in 0..6 {
            let _ = expected.next_u32();
        }
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    };
    let pool = |sim: &Simulation, master: u64| -> Vec<u64> {
        sim.substrate
            .entities
            .get(master)
            .unwrap()
            .slave_manager
            .as_ref()
            .unwrap()
            .slaves()
            .collect()
    };
    let alive = |sim: &Simulation, id: u64| {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .lifecycle
            .object_alive()
    };
    let smin = sim
        .spawn_object_at_height("SMIN", "YuriCountry", 10, 10, 0, 0, &rules)
        .expect("spawn SMIN");
    constructed(&sim, &mut expected);
    let slaves = pool(&sim, smin);
    assert_eq!(slaves, vec![2, 3, 4, 5, 6]);

    assert!(
        sim.deploy_mcv(
            smin,
            &rules,
            None,
            crate::sim::world::FrameEffects::default()
        ),
        "deploy to YAREFN"
    );
    let yarefn = sim
        .substrate
        .entities
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "YAREFN")
        .expect("deployed YAREFN")
        .stable_id();
    constructed(&sim, &mut expected);
    assert_eq!(pool(&sim, yarefn), slaves);
    // 0x006B0D10: the idle manager moves to 4 on the way.
    let state = sim
        .substrate
        .entities
        .get(yarefn)
        .unwrap()
        .slave_manager
        .as_ref()
        .unwrap()
        .state();
    assert_eq!(state, ManagerState::Deployed);
    assert!(
        sim.substrate
            .entities
            .get(smin)
            .unwrap()
            .slave_manager
            .is_none()
    );
    assert!(
        (8..=12).all(|fresh| !alive(&sim, fresh)),
        "the refinery's fresh slaves freed"
    );
    for &slave in &slaves {
        assert_eq!(
            sim.substrate.entities.get(slave).unwrap().slave.owner(),
            Some(yarefn)
        );
    }

    // Built up (`building_up` done), as `undeploy_building` requires, and
    // elite: the unit takes the refinery's VeterancyClass (`0x0044A058`).
    let refinery = sim.substrate.entities.get_mut(yarefn).unwrap();
    refinery.finish_building_construction_for_test();
    refinery.veterancy.set_elite();
    assert!(
        sim.undeploy_building(
            yarefn,
            &rules,
            Some(registry()),
            crate::sim::world::FrameEffects::default()
        ),
        "undeploy to SMIN"
    );
    sim.substrate
        .entities
        .get_mut(yarefn)
        .unwrap()
        .finish_pack_up_for_test();
    sim.visit_building_down(
        yarefn,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(
        std::mem::take(&mut sim.mission_spawned_entities),
        "converted"
    );
    constructed(&sim, &mut expected);
    let back = 13;
    assert_eq!(
        sim.interner
            .resolve(sim.substrate.entities.get(back).unwrap().type_ref()),
        "SMIN"
    );
    let unit = sim.substrate.entities.get(back).unwrap();
    let elite = sim.substrate.entities.get(yarefn).unwrap().veterancy.raw();
    assert_eq!((unit.veterancy.raw(), unit.veterancy.rank()), (elite, 200));
    // The rank cache (`+0x13C`) is not copied: the constructor's -1.
    assert_eq!(unit.veterancy.rank_cache(), -1);
    assert_eq!(pool(&sim, back), slaves);
    assert!(
        (14..=18).all(|fresh| !alive(&sim, fresh)),
        "the Slave Miner's fresh slaves freed"
    );
    for slave in slaves {
        assert_eq!(
            sim.substrate.entities.get(slave).unwrap().slave.owner(),
            Some(back)
        );
    }
}

/// A refinery with no ore within `SlaveMinerShortScan` of its centre and a
/// field farther out (state 5's relocation test): it archives its own cell
/// and packs up (the undeploy), the Slave Miner it becomes takes the manager
/// and its slaves, recalls them (state 6), hunts, deploys beside the field
/// and, once built up, lets the slaves out there.
#[test]
fn a_refinery_whose_ore_runs_out_packs_up_and_moves_to_the_next_field() {
    let inside = serde_json::json!({"state": 0, "cell": [13, 13], "limbo": true});
    let mut s = row_scene(&serde_json::json!({
        "name": "relocate",
        "human": false,
        "manager_state": 0,
        "slave_ranges": [8, 14, 48, 3],
        "nodes": [inside, inside, inside],
        "ore": [[24, 13, 0, 0, 5], [25, 13, 0, 0, 5], [24, 14, 0, 0, 5], [25, 14, 0, 0, 5]],
    }));
    s.scene.sim.ore_growth_config = OreGrowthConfig::disabled();
    let first = s.master;
    let slaves: Vec<u64> = s.slaves.values().copied().collect();
    let holder_of = |s: &SlaveScene| -> Option<u64> {
        s.scene
            .sim
            .substrate
            .entities
            .values()
            .find(|entity| entity.lifecycle.object_alive() && entity.slave_manager.is_some())
            .map(|entity| entity.stable_id())
    };
    let mut packed = false;
    let mut miner = None;
    let mut relocated = None;
    for _ in 0..2000 {
        frame(&mut s);
        let sim = &s.scene.sim;
        if let Some(refinery) = sim.substrate.entities.get(first)
            && refinery.building_down()
            && !packed
        {
            packed = true;
            assert_eq!(
                refinery.archive_target(),
                Some(crate::sim::combat::TargetKind::Cell(12, 12)),
                "the relocation archives the refinery's own cell"
            );
            assert_eq!(manager_state_of(&s, first), ManagerState::PackingUp);
        }
        let Some(holder) = holder_of(&s) else {
            continue;
        };
        let kind = sim
            .interner
            .resolve(sim.substrate.entities.get(holder).unwrap().type_ref());
        if kind == "SMIN" {
            if miner.is_none() {
                // `BuildingClass::Mission_Selling 0x0044A091..0x0044A0AE`: the
                // unit is sent to the refinery's archived cell, its own.
                let unit = sim.substrate.entities.get(holder).unwrap();
                assert_eq!(unit.navigation.nav_com(), Some(NavTargetRef::cell(12, 12)));
                assert_eq!(unit.mission.queued().known(), Some(MissionType::Move));
                assert_eq!(manager_state_of(&s, holder), ManagerState::PackingUp);
                let manager = unit.slave_manager.as_ref().unwrap();
                assert_eq!(manager.slaves().collect::<Vec<_>>(), slaves);
            }
            miner.get_or_insert(holder);
        } else if holder != first
            && slaves.iter().all(|&slave| {
                sim.substrate
                    .entities
                    .get(slave)
                    .is_some_and(|entity| !entity.lifecycle.in_limbo())
            })
        {
            relocated = Some(holder);
            break;
        }
    }
    assert!(packed, "the refinery packs up");
    let miner = miner.expect("the refinery becomes a Slave Miner holding the manager");
    let relocated = relocated.expect("the Slave Miner redeploys and lets its slaves out");
    let sim = &s.scene.sim;
    assert!(
        !sim.substrate
            .entities
            .get(first)
            .is_some_and(|entity| entity.lifecycle.object_alive()),
        "the first refinery is gone"
    );
    assert!(
        !sim.substrate
            .entities
            .get(miner)
            .is_some_and(|entity| entity.lifecycle.object_alive()),
        "the Slave Miner deployed"
    );
    let at = sim.substrate.entities.get(relocated).unwrap();
    assert_eq!(sim.interner.resolve(at.type_ref()), "YAREFN");
    assert!(
        (i32::from(at.position.rx()) - 24).abs() <= 3
            && (i32::from(at.position.ry()) - 13).abs() <= 3,
        "redeployed beside the field: ({}, {})",
        at.position.rx(),
        at.position.ry()
    );
    for &slave in &slaves {
        assert_eq!(
            sim.substrate.entities.get(slave).unwrap().slave.owner(),
            Some(relocated)
        );
    }
}

/// Retail `rulesmd.ini` and `artmd.ini` through the production reader
/// (skipped without them): what the relocation reads. `SlaveMinerShortScan`,
/// `SlaveMinerLongScan` and `SlaveMinerScanCorrection` (ReadRange leptons,
/// retail 8, 48 and 3 cells); YAREFN packs up into SMIN, plays
/// `SlaveMinerUndeploy` as it starts, and is 2x2 (no FindDeployCell step).
#[test]
fn retail_rules_feed_the_slave_refinery_relocation() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let general = &rules.general;
    assert_eq!(
        (
            general.slave_miner_short_scan,
            general.slave_miner_long_scan,
            general.slave_miner_scan_correction
        ),
        (8 * 256, 48 * 256, 3 * 256)
    );
    let refinery = rules.object("YAREFN").expect("YAREFN");
    assert_eq!(refinery.undeploys_into.as_deref(), Some("SMIN"));
    assert_eq!(refinery.deploy_sound.as_deref(), Some("SlaveMinerUndeploy"));
    assert_eq!(
        crate::rules::foundation::foundation_dimensions(&refinery.foundation),
        (2, 2)
    );
    let miner = rules.object("SMIN").expect("SMIN");
    assert_eq!(miner.deploys_into.as_deref(), Some("YAREFN"));
}

/// `BuildingClass::Mission_Selling`'s conversion constructs the unit, lists the
/// building's attackers (`0x00449F23..0x00449FDC`), and Limbos the building,
/// whose Detach_All clears their targets (an attacker with more than 10
/// frames left on its passive scan re-arms it with a Scenario draw); after
/// the unit's Unlimbo the listed attackers target it
/// (`0x0044A146..0x0044A167`), so an attacker keeps shooting at the Slave
/// Miner. Draws: the Slave Miner's constructor words, then the re-arm.
#[test]
fn an_attacker_of_a_packing_refinery_takes_the_slave_miner() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::combat::TargetKind;
    use crate::sim::rng::SimRng;
    use crate::sim::world::Simulation;
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[InfantryTypes]\n1=SLAV\n[VehicleTypes]\n1=SMIN\n2=TANK\n[BuildingTypes]\n1=YAREFN\n\
         [SLAV]\nStrength=125\nSpeed=3\nSlaved=yes\nStorage=4\n\
         [SMIN]\nStrength=2000\nSpeed=3\nEnslaves=SLAV\nSlavesNumber=1\nDeploysInto=YAREFN\n\
         [TANK]\nStrength=300\nSpeed=5\n\
         [YAREFN]\nStrength=2000\nEnslaves=SLAV\nSlavesNumber=1\nUndeploysInto=SMIN\n",
        ),
        &IniFile::from_str("[YAREFN]\nFoundation=2x2\n"),
    )
    .expect("rules");
    let mut rules = rules;
    rules.set_buildup_control_for_test("YAREFN", [0, 17, 3]);
    let seed = 0x0A77_AC4E;
    let mut sim = Simulation::with_seed(seed);
    let refinery = sim
        .spawn_object_at_height("YAREFN", "YuriCountry", 10, 10, 0, 0, &rules)
        .expect("YAREFN");
    let tank = sim
        .spawn_object_at_height("TANK", "Americans", 16, 10, 0, 0, &rules)
        .expect("TANK");
    let frame = sim.session.binary_frame();
    let attacker = sim.substrate.entities.get_mut(tank).unwrap();
    attacker
        .attack_target
        .set_for_test(Some(TargetKind::Entity(refinery)));
    attacker.passive_scan.timer_mut_for_test().arm(frame, 100);
    let scan_timer = attacker.passive_scan.timer();
    sim.substrate
        .entities
        .get_mut(refinery)
        .unwrap()
        .finish_building_construction_for_test();
    let mut expected = SimRng::new(seed);
    while expected.logical_state() != sim.scenario_rng.logical_state() {
        let _ = expected.next_u32();
    }

    assert!(sim.undeploy_building(
        refinery,
        &rules,
        Some(registry()),
        crate::sim::world::FrameEffects::default()
    ));
    sim.substrate
        .entities
        .get_mut(refinery)
        .unwrap()
        .finish_pack_up_for_test();
    sim.visit_building_down(
        refinery,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(
        std::mem::take(&mut sim.mission_spawned_entities),
        "converted"
    );
    let miner = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.lifecycle.object_alive() && sim.interner.resolve(entity.type_ref()) == "SMIN"
        })
        .expect("the Slave Miner")
        .stable_id();
    let attacker = sim.substrate.entities.get(tank).unwrap();
    assert_eq!(
        attacker.attack_target.target(),
        Some(TargetKind::Entity(miner))
    );
    // The Slave Miner's TechnoClass word and its one slave's, then the
    // passive-scan re-arm at the building's Limbo.
    for _ in 0..2 {
        let _ = expected.next_u32();
    }
    let delay = expected.next_range_u32_inclusive(4, 8);
    let mut rearmed = scan_timer;
    rearmed.arm(frame, delay);
    assert_eq!(attacker.passive_scan.timer(), rearmed);
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

#[test]
fn a_player_cell_order_relocates_a_refinery_with_its_existing_slave_manager() {
    for ore in [false, true] {
        let mut s = row_scene(&serde_json::json!({
            "name": "player_relocate", "human": true, "manager_state": 5,
            "nodes": [{"state": 3, "cell": [17, 13], "mission": "harvest"}],
            // Keep the old site productive: native Working's separate auto
            // relocation can overwrite an archive when the old field is empty.
            "ore": if ore { serde_json::json!([[17,13,0,0,5],[24,13,0,0,5]]) }
                else { serde_json::json!([[17,13,0,0,5]]) },
        }));
        s.scene.sim.ore_growth_config = OreGrowthConfig::disabled();
        s.scene.sim.input_delay_ticks = 0;
        s.scene
            .rules
            .set_buildup_control_for_test("YAREFN", [0, 17, 3]);
        let old = s.master;
        let slave = s.slaves[&0];
        let owner = s.scene.sim.entities().get(old).unwrap().owner();
        assert!(
            s.scene
                .sim
                .takes_rally_point(old, "Americans", &s.scene.rules)
        );
        let goal = s
            .scene
            .sim
            .factory_rally_cell_input(old, (24, 13), &s.scene.rules)
            .unwrap()
            .unwrap();
        assert_eq!(goal, (24, 13));
        let execute = s.scene.sim.session.tick() + 1;
        for command in [
            Command::SetRally {
                rx: goal.0,
                ry: goal.1,
                producer_ids: vec![old],
            },
            Command::SellBuilding { entity_id: old },
        ] {
            s.scene
                .sim
                .queue_command(CommandEnvelope::new(owner, execute, command));
        }
        frame(&mut s);
        let actor = s.scene.sim.entities().get(old).unwrap();
        assert!(actor.building_down(), "player SELL must start immediately");
        if !ore {
            assert_eq!(
                actor.archive_target(),
                Some(crate::sim::combat::TargetKind::Cell(goal.0, goal.1))
            );
        }
        let mut converted = None;
        for _ in 0..800 {
            frame(&mut s);
            if let Some(unit) = s.scene.sim.entities().values().find(|entity| {
                entity.is_object_alive()
                    && entity.slave_manager.is_some()
                    && entity.category() == crate::map::entities::EntityCategory::Unit
            }) {
                converted = Some(unit.stable_id());
                break;
            }
        }
        let converted = converted.expect("SELL must convert through the production frame");
        let sim = &s.scene.sim;
        assert!(
            sim.entities()
                .get(old)
                .is_none_or(|entity| !entity.is_object_alive())
        );
        let unit = sim.entities().get(converted).unwrap();
        let destination = unit
            .navigation
            .nav_com()
            .expect("converted unit keeps the archived goal");
        assert_eq!(unit.mission.effective().known(), Some(MissionType::Move));
        if !ore {
            assert_eq!(destination, NavTargetRef::cell(goal.0, goal.1));
        } else {
            assert_ne!(
                destination,
                NavTargetRef::cell(goal.0, goal.1),
                "FindDeployCell avoids the ore overlay for the new foundation"
            );
            assert!(
                matches!(
                    unit.slave_manager.as_ref().unwrap().state(),
                    ManagerState::PackingUp | ManagerState::Scanning
                ),
                "the converted Unit may already run the native PackingUp -> Scanning visit"
            );
        }
        assert_eq!(
            unit.slave_manager
                .as_ref()
                .unwrap()
                .slaves()
                .collect::<Vec<_>>(),
            [slave]
        );
        assert_eq!(
            sim.entities().get(slave).unwrap().slave.owner(),
            Some(converted)
        );
    }
}

/// FootClass::Mission_AreaGuard starts an idle Slave Miner's hunt
/// (`0x004D6D69..0x004D6D73`, StartHunt `0x006B0CC0`) on every visit the
/// Unit kick (`0x00744100`) does not take, so a fresh Area Guard (the G key,
/// or an AI team's script 11) sends the miner out at once. The kick alone
/// waits `SlaveMinerKickFrameDelay` (150) after the mission began and, for a
/// human, needs ore within `SlaveMinerShortScan` (8 cells); this ore is 10
/// cells away, inside `SlaveMinerLongScan`.
#[test]
fn area_guard_starts_an_idle_slave_miners_hunt_at_once() {
    for human in [true, false] {
        let mut s = row_scene(&serde_json::json!({
            "name": "area_guard_hunt",
            "owner": "unit",
            "owner_mission": "area_guard",
            "human": human,
            "manager_state": 0,
            "manager_frame_ago": 0,
            "ore": [[25, 15, 0, 0, 5], [25, 16, 0, 0, 5]],
        }));
        assert_eq!(manager_state_of(&s, s.master), ManagerState::Ready);
        let mut states = vec![ManagerState::Ready];
        for _ in 0..30 {
            frame(&mut s);
            let state = manager_state_of(&s, s.master);
            if states.last() != Some(&state) {
                states.push(state);
            }
        }
        // StartHunt (state 1), then state 1 finds the field and drives.
        assert_eq!(
            states,
            [
                ManagerState::Ready,
                ManagerState::Scanning,
                ManagerState::Travelling
            ],
            "human {human}"
        );
        let miner = s.scene.sim.substrate.entities.get(s.master).unwrap();
        assert!(miner.navigation.nav_com().is_some(), "human {human}");
    }
}
