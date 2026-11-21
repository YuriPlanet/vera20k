//! Attack and guard order tests: attack-move pathing, targets and kills on the way,
//! lethal-hit restore and stun, attackers detached from a stunned building, and the
//! area-guard leash.

use super::*;

fn teleport_command_test_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=CMIN\n1=CHRONO\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GAREFN\n\n\
         [CMIN]\nStrength=400\nArmor=light\nSpeed=4\nHarvester=yes\nTeleporter=yes\nDock=GAREFN\n\n\
         [CHRONO]\nStrength=200\nArmor=light\nSpeed=5\nTeleporter=yes\n\
         Locomotor={4A582747-9839-11d1-B709-00A024DDAFD1}\n\n\
         [GAREFN]\nStrength=900\nArmor=wood\nRefinery=yes\nDockUnload=yes\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(
        &ini,
        &IniFile::from_str("[GAREFN]\nFoundation=4x3\n"),
    )
    .expect("teleport command rules should parse")
}

#[test]
fn test_attack_move_command_chrono_miner_uses_ground_path() {
    let rules = teleport_command_test_rules();
    let mut sim: Simulation = Simulation::new();
    let entity = sim
        .spawn_object("CMIN", "Americans", 2, 2, 64, &rules)
        .expect("spawn chrono miner");
    let grid = PathGrid::new(32, 32);
    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::AttackMove {
            entity_id: entity,
            target: None,
            destination: Some(crate::sim::combat::TargetKind::Cell(8, 2)),
        },
    );

    let _ = sim.advance_tick(&[cmd], Some(&rules), Some(&grid), None, 33);
    assert_eq!(
        sim.substrate
            .entities
            .get(entity)
            .and_then(crate::sim::movement::motion_query::is_moving),
        Some(true),
        "Chrono Miner should path on attack-move instead of teleporting"
    );
    // No `Primary=` refuses the attack-move (`0x00711E90`), so
    // Assign_Target_Command answers Move and stores nothing.
    let miner = sim.substrate.entities.get(entity).unwrap();
    assert!(!miner.seldom.mega_mission.is_attack_move());
    assert_eq!(
        miner.mission.queued(),
        MissionId::from_known(MissionType::Move),
        "a Chrono Miner's attack-move is a plain Move"
    );
    assert!(
        sim.substrate
            .entities
            .get(entity)
            .and_then(|e| e.teleport_state())
            .is_none(),
        "Chrono Miner should not enter teleport movement on attack-move"
    );
}

/// An American MTNK (1) at (20, 20) and a Russian E1 (2) two cells east of
/// it, in the playfield.
fn attack_move_fight(rules: &RuleSet) -> Simulation {
    let mut sim: Simulation = Simulation::new();
    // The Unit setter's Move_To needs map cells, and the threat scan's ring
    // walk skips cells outside the Map Size diamond: the fight is inside it.
    install_rectangular_test_map(&mut sim, 32, 32);
    sim.install_resolved_terrain_for_new_map(gsi_04_10_clear_terrain(64, 64));
    sim.spawn_from_map(
        &[
            MapEntity {
                owner: "Americans".to_string(),
                type_id: "MTNK".to_string(),
                health: 256,
                cell_x: 20,
                cell_y: 20,
                facing: 64,
                category: EntityCategory::Unit,
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
            },
            MapEntity {
                owner: "Russians".to_string(),
                type_id: "E1".to_string(),
                health: 256,
                cell_x: 22,
                cell_y: 20,
                facing: 64,
                category: EntityCategory::Infantry,
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
            },
        ],
        Some(rules),
    );
    assert!(sim.rebuild_dynamic_navigation(rules));
    // This spawn leaves IsInPlayfield (`+0x3D5`) clear; the scan reads it.
    for id in [1, 2] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .set_in_playfield(true);
    }
    sim
}

/// The victim plays a Die1 sequence: a zero-count death request is refused
/// (`0x0051D70F`) and the next visit's Health reset (`0x0051BC57`) would
/// revive it at Health 1 for the attack-move to acquire again.
#[test]
fn test_attack_move_lethal_hit_expires_the_target_at_the_kill() {
    let rules = walking_combat_test_rules();
    let mut sim = attack_move_fight(&rules);
    // The player has seen the enemy: the campaign scan gate needs +0x41B.
    crate::sim::arena_fixture::discover(&mut sim, 2);
    if let Some(e) = sim.substrate.entities.get_mut(2) {
        e.health.set_for_test(50);
    }
    let grid = PathGrid::new(64, 64);
    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::AttackMove {
            entity_id: 1,
            target: None,
            destination: Some(crate::sim::combat::TargetKind::Cell(30, 20)),
        },
    );

    let _ = sim.advance_tick(&[cmd], Some(&rules), Some(&grid), None, 100);
    // The tail-dispatched AttackMove cannot take part in the object-AI walk
    // that preceded it. It acquires once its targeting timer runs out, and
    // its first hit kills.
    for _ in 0..200 {
        if sim.substrate.entities.get(2).unwrap().health.current() == 0 {
            break;
        }
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
    }
    let victim = sim
        .substrate
        .entities
        .get(2)
        .expect("animated victim remains stored through its death sequence");
    assert_eq!(victim.health.current(), 0);
    assert!(!victim.death.is_dying());
    assert!(victim.lifecycle.object_alive());

    let attacker = sim.substrate.entities.get(1).expect("attacker exists");
    assert!(
        attacker.attack_target.target().is_none(),
        "ObjectClass::ReceiveDamage's exact-zero Destroy (0x005F57AF) expires the target at the kill"
    );
    assert_eq!(
        attacker.seldom.mega_mission.destination(),
        Some(crate::sim::combat::TargetKind::Cell(30, 20)),
        "the attack-move order outlives its target"
    );
    assert!(attacker.seldom.mega_mission.is_attack_move());
    // Mission_Attack, finding no target, enters idle mode, whose
    // RefreshMegaMission resumes the order: Move to the MegaDestination.
    let mut resumed = false;
    for _ in 0..60 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        let attacker = sim.substrate.entities.get(1).expect("attacker exists");
        if attacker.mission.effective().known() == Some(MissionType::Move) {
            assert_eq!(
                attacker.navigation.nav_com(),
                Some(crate::sim::components::NavTargetRef::cell(30, 20))
            );
            assert!(attacker.seldom.mega_mission.is_attack_move());
            resumed = true;
            break;
        }
    }
    assert!(resumed, "the attack-move resumes towards its destination");
}

/// An attack-move click on an enemy sends it as the event target, which
/// `Assign_Target_Command` stores as MegaTarget and answers Attack
/// (`0x004DF12A..0x004DF186`). The event hands it to the class target setter
/// (`0x004C7467`) and its NULL destination to the destination setter
/// (`0x004C747C`).
#[test]
fn an_attack_move_click_on_an_enemy_attacks_it_as_the_mega_target() {
    let rules = driving_combat_test_rules();
    let mut sim = attack_move_fight(&rules);
    assert!(sim.apply_command(
        "Americans",
        &Command::AttackMove {
            entity_id: 1,
            target: Some(crate::sim::combat::TargetKind::Entity(2)),
            destination: None,
        },
        Some(&rules),
    ));
    let attacker = sim.substrate.entities.get(1).unwrap();
    assert!(attacker.seldom.mega_mission.is_attack_move());
    assert!(attacker.seldom.mega_mission.targets(2));
    assert_eq!(attacker.seldom.mega_mission.destination(), None);
    assert_eq!(
        attacker.mission.queued(),
        MissionId::from_known(MissionType::Attack)
    );
    assert_eq!(
        attacker.attack_target.target(),
        Some(crate::sim::combat::TargetKind::Entity(2))
    );
    assert_eq!(attacker.navigation.nav_com(), None);

    // A destination deleted before the event runs still answers Move, with a
    // NULL MegaDestination; the MegaTarget keeps its value
    // (`0x004DF0F7..0x004DF146`).
    assert!(sim.apply_command(
        "Americans",
        &Command::AttackMove {
            entity_id: 1,
            target: None,
            destination: Some(crate::sim::combat::TargetKind::Entity(99)),
        },
        Some(&rules),
    ));
    let attacker = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        attacker.mission.queued(),
        MissionId::from_known(MissionType::Move)
    );
    assert_eq!(attacker.seldom.mega_mission.destination(), None);
    assert!(attacker.seldom.mega_mission.targets(2));
}

/// An attack-move click over an object sends it as the event destination,
/// which `Assign_Target_Command` stores as MegaDestination and answers Move
/// (`0x004DF12A..0x004DF186`); the class destination setter takes it as the
/// NavCom (`0x004C747C`). When the object expires, Foot PointerExpired drops
/// both (`0x004D9A0F..0x004D9ABD`, `0x004D9B35..0x004D9B3D`) and the order
/// stays an attack-move. A dead destination rejects the whole event
/// (`0x004C7263..0x004C7299`).
#[test]
fn an_attack_move_click_over_an_object_keeps_it_the_mega_destination() {
    let rules = driving_combat_test_rules();
    let mut sim = attack_move_fight(&rules);
    let mcv = sim
        .spawn_object("AMCV", "Americans", 18, 20, 0, &rules)
        .unwrap();
    let attack_move_onto = |sim: &mut Simulation, id| {
        sim.apply_command(
            "Americans",
            &Command::AttackMove {
                entity_id: 1,
                target: None,
                destination: Some(crate::sim::combat::TargetKind::Entity(id)),
            },
            Some(&rules),
        )
    };

    assert!(attack_move_onto(&mut sim, mcv));
    let attacker = sim.substrate.entities.get(1).unwrap();
    assert!(attacker.seldom.mega_mission.is_attack_move());
    assert_eq!(
        attacker.seldom.mega_mission.destination(),
        Some(crate::sim::combat::TargetKind::Entity(mcv))
    );
    assert_eq!(
        attacker.mission.queued(),
        MissionId::from_known(MissionType::Move)
    );
    assert_eq!(
        attacker.navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::Object { id: mcv })
    );

    sim.detach_all_pointer_expired(mcv, &rules, &mut crate::sim::world::FrameEffects::default());
    let attacker = sim.substrate.entities.get(1).unwrap();
    assert_eq!(attacker.seldom.mega_mission.destination(), None);
    assert_eq!(attacker.navigation.nav_com(), None);
    assert!(attacker.seldom.mega_mission.is_attack_move());

    sim.substrate
        .entities
        .get_mut(mcv)
        .unwrap()
        .health
        .set_for_test(0);
    assert!(!attack_move_onto(&mut sim, mcv));
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .mega_mission
            .destination(),
        None,
        "the refused event stores nothing"
    );

    // A target deleted before the event runs still answers Attack, with a
    // NULL MegaTarget (`0x004DF0F7..0x004DF164`).
    assert!(sim.apply_command(
        "Americans",
        &Command::AttackMove {
            entity_id: 1,
            target: Some(crate::sim::combat::TargetKind::Entity(99)),
            destination: None,
        },
        Some(&rules),
    ));
    let attacker = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        attacker.mission.queued(),
        MissionId::from_known(MissionType::Attack)
    );
    assert!(attacker.seldom.mega_mission.is_attack_move());
    assert!(attacker.attack_target.target().is_none());
}

/// An infantryman's attack-move through `advance_tick`: the scan on its
/// targeting timer finds an enemy (Arrival_Target_Handler, `0x004DF3A0`), its
/// queued Attack commences when Infantry readiness allows
/// (`InfantryClass::Is_Ready_To_Commence @ 0x00521B60`), and once the enemy
/// dies, Enter_Idle_Mode's RefreshMegaMission (`0x004DF4B0`) sends him on to
/// the ordered cell. There the next idle entry ends the order: Get_Mission is
/// Move, not Guard, so Clear_All_TarCom (`0x004DF1A0`).
#[test]
fn test_infantry_attack_move_kills_on_the_way_and_walks_on() {
    let rules = walking_combat_test_rules();
    let mut sim: Simulation = Simulation::new();
    install_rectangular_test_map(&mut sim, 32, 32);
    sim.install_resolved_terrain_for_new_map(gsi_04_10_clear_terrain(64, 64));
    let infantry = |owner: &str, cell_x: u16| MapEntity {
        owner: owner.to_string(),
        cell_x,
        cell_y: 20,
        ..make_test_entity("E1", EntityCategory::Infantry)
    };
    sim.spawn_from_map(
        &[infantry("Americans", 20), infantry("Russians", 24)],
        Some(&rules),
    );
    // The player has seen the enemy: the campaign scan gate needs +0x41B.
    crate::sim::arena_fixture::discover(&mut sim, 2);
    // Infantry fear asks the owner's House whether a human controls it.
    for (owner, side_index, is_human) in [("Americans", 0, true), ("Russians", 1, false)] {
        let name = sim.interner.intern(owner);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                name, side_index, None, is_human, 0, 10,
            ));
    }
    assert!(sim.rebuild_dynamic_navigation(&rules));
    // This spawn leaves IsInPlayfield (`+0x3D5`) clear; the scan reads it.
    for id in [1, 2] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .set_in_playfield(true);
    }
    // One M60 burst (25) kills.
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .health
        .set_for_test(20);
    let grid = PathGrid::new(64, 64);
    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::AttackMove {
            entity_id: 1,
            target: None,
            destination: Some(crate::sim::combat::TargetKind::Cell(30, 20)),
        },
    );
    let _ = sim.advance_tick(&[cmd], Some(&rules), Some(&grid), None, 100);
    assert!(sim.mega_mission(1).is_attack_move());

    let mut acquired = false;
    let mut arrived = false;
    for _ in 0..600 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        let attacker = sim.substrate.entities.get(1).expect("attacker lives");
        acquired |=
            attacker.attack_target.target() == Some(crate::sim::combat::TargetKind::Entity(2));
        let victim_dead = sim
            .substrate
            .entities
            .get(2)
            .is_none_or(|victim| victim.health.current() == 0);
        if victim_dead
            && (attacker.position.rx(), attacker.position.ry()) == (30, 20)
            && attacker.navigation.nav_com().is_none()
            && !attacker.seldom.mega_mission.is_attack_move()
        {
            arrived = true;
            break;
        }
    }
    assert!(acquired, "the attack-move scan finds the enemy");
    assert!(
        arrived,
        "the enemy dies, the order resumes to its cell and ends there"
    );
}

/// `TechnoClass::PointerExpired @ 0x007077C0` clears a listener's Target and,
/// with a mission suspended, Restores it. The archived Target re-enters
/// `Assign_Target @ 0x006FCDB0`, which commits NULL for a Health-0 object
/// (`0x006FCEF8..0x006FCF03`). AI retaliation archives the target a unit
/// already shoots, so a killer whose current and archived targets are both its
/// victim holds no target once the killing hit's Destroy has run, while the
/// infantry corpse is still stored for its die sequence.
#[test]
fn test_lethal_hit_restore_refuses_the_dying_archived_target() {
    let rules = combat_test_rules();
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[
            MapEntity {
                cell_x: 2,
                cell_y: 2,
                ..make_test_entity("MTNK", EntityCategory::Unit)
            },
            MapEntity {
                owner: "Russians".to_string(),
                cell_x: 4,
                cell_y: 2,
                ..make_test_entity("E1", EntityCategory::Infantry)
            },
        ],
        Some(&rules),
    );
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .health
        .set_for_test(50);
    {
        let tank = sim.substrate.entities.get_mut(1).unwrap();
        tank.attack_target.set_for_test(Some(TargetKind::Entity(2)));
        tank.seldom
            .suspended_attack_target
            .set_for_test(Some(crate::sim::combat::TargetKind::Entity(2)));
        tank.mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: MissionId::from_known(MissionType::Attack),
                suspended: MissionId::from_known(MissionType::Guard),
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
            });
    }
    let grid = PathGrid::new(32, 32);
    for _ in 0..8 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        if sim
            .substrate
            .entities
            .get(2)
            .is_some_and(|victim| victim.health.current() == 0)
        {
            break;
        }
    }

    let victim = sim
        .substrate
        .entities
        .get(2)
        .expect("the corpse stays stored for its die sequence");
    assert_eq!(victim.health.current(), 0);
    assert!(victim.lifecycle.object_alive());
    let tank = sim.substrate.entities.get(1).expect("the killer survives");
    assert!(
        tank.attack_target.target().is_none(),
        "Assign_Target refuses the dying archived target the Restore names"
    );
    assert_eq!(tank.mission.current().known(), Some(MissionType::Guard));
    assert_eq!(tank.seldom.suspended_attack_target.target(), None);
}

/// The death arm's Stun (`0x00702210`: FootClass::Stun `0x004D5660`, then
/// TechnoClass::Stun `0x006FCD40`) drops the dying object's own Target and
/// destination at the killing hit, while the infantry corpse is still stored
/// for its die sequence.
#[test]
fn test_lethal_hit_stuns_the_dying_infantry() {
    let rules = combat_test_rules();
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[
            MapEntity {
                owner: "Russians".to_string(),
                cell_x: 4,
                cell_y: 2,
                ..make_test_entity("E1", EntityCategory::Infantry)
            },
            MapEntity {
                cell_x: 2,
                cell_y: 2,
                ..make_test_entity("MTNK", EntityCategory::Unit)
            },
        ],
        Some(&rules),
    );
    {
        let gi = sim.substrate.entities.get_mut(1).unwrap();
        gi.attack_target.set_for_test(Some(TargetKind::Entity(2)));
        gi.navigation
            .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::Cell {
                rx: 9,
                ry: 2,
            }));
    }
    let warhead = sim.interner.intern("AP");
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        1,
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
    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[hit],
        &mut crate::sim::world::FrameEffects::default(),
    );

    let gi = sim
        .substrate
        .entities
        .get(1)
        .expect("the corpse stays stored for its die sequence");
    assert_eq!(gi.health.current(), 0);
    assert!(gi.lifecycle.object_alive());
    assert!(
        gi.attack_target.target().is_none(),
        "TechnoClass::Stun Assign_Target(NULL)"
    );
    assert!(
        gi.navigation.nav_com().is_none(),
        "FootClass::Stun Assign_Destination(NULL)"
    );
}

/// `TechnoClass::Stun @ 0x006FCD40` runs Kill_All_Spawns, ClearAllTargets
/// and Detach_All(1) for a non-Foot object too (`JZ 0x006FCD7B`): a stunned
/// building drops out of its attacker's Target.
#[test]
fn test_stunning_a_building_detaches_its_attackers() {
    let rules = combat_test_rules();
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[
            MapEntity {
                owner: "Russians".to_string(),
                cell_x: 6,
                cell_y: 2,
                ..make_test_entity("GACNST", EntityCategory::Structure)
            },
            MapEntity {
                cell_x: 2,
                cell_y: 2,
                ..make_test_entity("MTNK", EntityCategory::Unit)
            },
        ],
        Some(&rules),
    );
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(1)));
    sim.stun(1, UninitContext::new(&rules));
    assert_eq!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .attack_target
            .target(),
        None,
        "Detach_All(1) clears the tank's Target"
    );
}

#[test]
fn test_area_guard_keeps_its_post_when_displaced_inside_the_native_leash() {
    let rules = combat_test_rules();
    let mut sim: Simulation = Simulation::new();
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    sim.spawn_from_map(
        &[MapEntity {
            cell_x: 2,
            cell_y: 2,
            ..make_test_entity("MTNK", EntityCategory::Unit)
        }],
        Some(&rules),
    );
    let guard_cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Guard {
            entity_id: 1,
            target: Some(crate::sim::combat::TargetKind::Cell(2, 2)),
        },
    );
    let _ = sim.advance_tick(&[guard_cmd], Some(&rules), Some(&grid), None, 100);

    sim.remove_entity_occupancy(1);
    if let Some(e) = sim.substrate.entities.get_mut(1) {
        e.position.set_rx_for_test(5);
        e.position.set_ry_for_test(2);
        e.attack_target.set_for_test(None);
        e.navigation.set_nav_com_for_test(None);
    }
    sim.add_entity_occupancy(1);

    let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
    let ge = sim
        .substrate
        .entities
        .get(1)
        .expect("entity 1 should exist");
    assert!(!ge.seldom.mega_mission.is_attack_move());
    assert_eq!(
        ge.archive_target(),
        Some(crate::sim::combat::TargetKind::Cell(2, 2))
    );
    assert!(
        ge.navigation.nav_com().is_none(),
        "a displacement within the leash does not force a return"
    );
}
