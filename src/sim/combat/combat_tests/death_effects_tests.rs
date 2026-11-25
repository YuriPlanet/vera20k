//! What a death sets off: death weapons, postmortem callbacks, death and
//! detonation anims, and debris.

use super::*;

// Phobos `ROF.RandomDelay` (`0x006FD0B5`) redraws the full-rearm jitter, so
// this stock golden only holds without the capability.
#[cfg(not(feature = "phobos"))]
#[test]
fn gsi_04_07_damage_prior_projectile_fatal_death_weapon_is_inline() {
    fn run(victim_hp: i32, explodes: bool) -> (CombatTickResult, OverlayGrid, EntityStore, u64) {
        let ini_text = format!(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=BOOMER\n1=SHOOTER\n2=TARGET\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [OverlayTypes]\n0=TESTWALL\n\
             [BOOMER]\nStrength=11\nArmor=heavy\nExplodes={}\nDeathWeapon=DeathBoom\n\
             [SHOOTER]\nStrength=100\nArmor=heavy\nPrimary=Gun\n\
             [TARGET]\nStrength=100\nArmor=heavy\n\
             [DeathBoom]\nDamage=214\nWarhead=WallWH\n\
             [Gun]\nDamage=0\nROF=50\nRange=8\nWarhead=NoWallWH\n\
             [WallWH]\nCellSpread=0\nWall=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [NoWallWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
            if explodes { "yes" } else { "no" },
        );
        let ini = IniFile::from_str(&ini_text);
        let art = IniFile::from_str("[TESTWALL]\nDamageLevels=2\n");
        let rules = RuleSet::from_ini(&ini).expect("inline death-weapon rules");
        let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));
        assert_eq!(registry.flags(0).unwrap().strength, 400);
        assert_eq!(registry.flags(0).unwrap().damage_levels, 2);
        assert_eq!(
            rules.object("BOOMER").unwrap().death_weapon.as_deref(),
            Some("DeathBoom")
        );
        assert!(!rules.warhead("WallWH").unwrap().wall_absolute_destroyer);
        assert_eq!(rules.weapon("DeathBoom").unwrap().damage, 214);

        let mut entities = EntityStore::new();
        entities.insert(make_entity(10, "BOOMER", 8, 5, victim_hp));
        // West of both the wall cell and the target restored mid-tick, so one
        // hull heading serves both — see the note in the sibling live-order
        // test: a restored target BEHIND a turretless hull is refused for
        // facing by `UnitClass::GetFireError @ 0x00740FD0` step 17.
        let mut later_attacker = make_entity(20, "SHOOTER", 4, 5, 100);
        set_mission(
            &mut later_attacker,
            MissionType::Attack,
            MissionId::from_known(MissionType::Guard),
        );
        later_attacker
            .seldom
            .suspended_attack_target
            .set_for_test(Some(TargetKind::Entity(30)));
        entities.insert(later_attacker);
        entities.insert(make_entity(30, "TARGET", 5, 5, 100));
        let mut interner = test_interner();
        let _handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
        assert!(install_cell_attack_target_for_test(
            &mut entities,
            20,
            8,
            5,
            Some(&rules),
            &interner,
        ));

        let mut overlays = OverlayGrid::new(16, 16);
        let mut terrain = crate::sim::tiberium::test_support::flat_terrain(16, 16);
        overlays.place_overlay(8, 5, 0, 0);
        let detonation = ProjectileDetonation {
            projectile_id: 1,
            source_id: 99,
            target: ProjectileTarget::Entity(10),
            impact: ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
            payload: payload_from_weapon(
                10,
                interner.intern("NoWallWH"),
                interner.intern("Gun"),
                &rules,
                &mut interner,
            ),
            reason: ProjectileDetonationReason::ReachedTarget,
        };
        let mut scenario_rng = SimRng::new(1);
        let mut main_rng = SimRng::new(41);
        let mut houses = crate::sim::houses::Houses::default();
        let handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
        align_attackers_to_targets(&mut entities);
        // With resolved terrain the detonation collects its victims from the
        // cell lists, so the objects are placed there as live ones are.
        for id in [10, 20, 30] {
            entities
                .get_mut(id)
                .unwrap()
                .lifecycle
                .set_cell_marked_for_test(true);
        }
        let mut occupancy = OccupancyGrid::rebuild(&entities);
        let result = tick_combat_with_fog_and_main_rng(
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            Some(handles),
            None,
            &BTreeMap::new(),
            &mut houses,
            &HouseAllianceMap::new(),
            None,
            Some(&mut overlays),
            Some(&registry),
            Some(&mut terrain),
            0,
            100,
            0,
            &[10, 20, 30],
            &[detonation],
            None,
            &mut scenario_rng,
            &mut main_rng,
            None,
        );
        (result, overlays, entities, scenario_rng.state())
    }

    let (fatal, fatal_grid, fatal_entities, fatal_rng) = run(10, true);
    assert_eq!(fatal_entities.get(10).unwrap().health.current(), 0);
    assert_eq!(fatal.consequences.effects().immediate_uninit_ids, vec![10]);
    assert_eq!(fatal_grid.cell(8, 5).overlay_id, None);
    assert_eq!(
        fatal
            .consequences
            .fire_events()
            .iter()
            .map(|event| (event.attacker_id, event.target))
            .collect::<Vec<_>>(),
        vec![(20, TargetKind::Entity(30))],
        "later live-order attacker must observe nested target restoration"
    );
    assert!(
        fatal
            .consequences
            .effects()
            .cell_target_detaches
            .iter()
            .any(|detach| { detach.listener_id == 20 && detach.restored && !detach.cleared })
    );
    assert_eq!(
        fatal_entities
            .get(20)
            .unwrap()
            .attack_target
            .target()
            .unwrap(),
        TargetKind::Entity(30)
    );
    // Detonation processing runs before the fire phase, so the inline wall
    // draw lands first and the surviving attacker's `GetROF` reload jitter
    // (`RandomRanged(0, 2)` @ `0x006FD0B0`) follows it.
    let mut one_draw = SimRng::new(1);
    assert_eq!(one_draw.next_range_u32_inclusive(0, 400), 213);
    one_draw.next_range_u32_inclusive(0, 2);
    assert_eq!(fatal_rng, one_draw.state(), "nested wall draw is inline");

    let (survives, surviving_grid, surviving_entities, surviving_rng) = run(11, true);
    // No wall draw when the wall survives — only the attacker's reload jitter.
    assert_eq!(surviving_rng, {
        let mut expected = SimRng::new(1);
        expected.next_range_u32_inclusive(0, 2);
        expected.state()
    });
    assert_eq!(surviving_grid.cell(8, 5).overlay_id, Some(0));
    assert!(survives.consequences.effects().wall_mutations.is_empty());
    assert!(
        survives
            .consequences
            .effects()
            .cell_target_detaches
            .is_empty()
    );
    assert!(
        survives
            .consequences
            .effects()
            .immediate_uninit_ids
            .is_empty()
    );
    assert_eq!(surviving_entities.get(10).unwrap().health.current(), 1);
    assert_eq!(
        survives
            .consequences
            .fire_events()
            .iter()
            .map(|event| (event.attacker_id, event.target))
            .collect::<Vec<_>>(),
        vec![(20, TargetKind::Cell(8, 5))],
        "HP=damage+1 must not expose death-wall or restored-target state"
    );

    let (ungated, ungated_grid, ungated_entities, ungated_rng) = run(10, false);
    assert_eq!(ungated_entities.get(10).unwrap().health.current(), 0);
    assert_eq!(ungated_grid.cell(8, 5).overlay_id, Some(0));
    assert_eq!(ungated_rng, {
        let mut expected = SimRng::new(1);
        expected.next_range_u32_inclusive(0, 2);
        expected.state()
    });
    assert!(ungated.consequences.effects().wall_mutations.is_empty());
    assert!(
        ungated
            .consequences
            .effects()
            .cell_target_detaches
            .is_empty()
    );
}

#[test]
fn gsi_04_07_damage_postmortem_stock_barrel_delay_and_nested_order() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=CAMISC02\n\
         [Warheads]\n0=OilExplosionWH\n1=Super\n2=BarrelWallWH\n\
         [OverlayTypes]\n0=TESTWALL\n\
         [CombatDamage]\nC4Warhead=Super\n\
         [CAMISC02]\nStrength=5\nArmor=concrete\nCanC4=no\nExplodes=yes\n\
         EligibleForDelayKill=yes\nDeathWeapon=BarrelExplosion\n\
         [OilExplosionWH]\nCellSpread=4\nPercentAtMax=.5\nCausesDelayKill=yes\n\
         DelayKillFrames=5\nDelayKillAtMax=7.0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Super]\nCellSpread=0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [BarrelExplosion]\nDamage=200\nWarhead=BarrelWallWH\n\
         [BarrelWallWH]\nCellSpread=0\nWall=yes\nWallAbsoluteDestroyer=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("stock-shaped PostMortem rules");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    let mut sim = crate::sim::world::Simulation::new();
    sim.resolve_type_handles(&rules);
    let center = sim
        .spawn_object("CAMISC02", "Neutral", 8, 5, 0, &rules)
        .expect("center barrel");
    let middle = sim
        .spawn_object("CAMISC02", "Neutral", 10, 5, 0, &rules)
        .expect("middle barrel");
    let edge = sim
        .spawn_object("CAMISC02", "Neutral", 12, 5, 0, &rules)
        .expect("edge barrel");
    sim.substrate
        .entities
        .get_mut(edge)
        .unwrap()
        .building
        .pending_c4_detonation
        .set_for_test(Some(crate::sim::c4_plant::PendingC4Detonation::new(
            crate::sim::timer::CdTimer::started(0, 40),
            Some(center),
        )));
    *sim.overlay_grid = Some(OverlayGrid::new(20, 12));
    *sim.resolved_terrain = Some(crate::sim::tiberium::test_support::flat_terrain(20, 12));
    sim.overlay_grid.as_mut().unwrap().place_overlay(8, 5, 0, 0);
    let oil_wh = sim.interner.intern("OilExplosionWH");

    sim.commit_noncombat_aoe_hits(
        &rules,
        Some(&registry),
        &[
            EntityDamageEvent::area(center, 10, 0, RAD_NO_ATTACKER, None, oil_wh),
            EntityDamageEvent::area(middle, 10, 512, RAD_NO_ATTACKER, None, oil_wh),
            EntityDamageEvent::area(edge, 10, 1024, RAD_NO_ATTACKER, None, oil_wh),
        ],
        &mut crate::sim::world::FrameEffects::default(),
    );

    let pending = |sim: &crate::sim::world::Simulation, id| {
        sim.substrate
            .entities
            .get(id)
            .and_then(|entity| entity.building.pending_c4_detonation.pending())
            .expect("qualifying fatal barrel becomes PostMortem")
    };
    assert_eq!(pending(&sim, center).timer().duration(), 5);
    assert_eq!(pending(&sim, middle).timer().duration(), 20);
    assert_eq!(pending(&sim, edge).timer().duration(), 35);
    assert_eq!(
        pending(&sim, edge).source_entity_id(),
        None,
        "center's exact-zero Destroy notification expires the retained source pointer"
    );
    for id in [center, middle, edge] {
        let barrel = sim.substrate.entities.get(id).unwrap();
        assert_eq!(barrel.health.current(), 1);
        assert!(barrel.lifecycle.object_alive() && !barrel.death.is_dying());
    }
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(8, 5).overlay_id,
        Some(0)
    );

    // IC/FS uses the Building wrapper and cancels the shared timer outright.
    crate::sim::superweapon::invulnerability::apply_invulnerability(
        sim.substrate.entities.get_mut(middle).unwrap(),
        0,
        30,
        crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
    );
    assert!(
        sim.substrate
            .entities
            .get(middle)
            .unwrap()
            .building
            .pending_c4_detonation
            .pending()
            .is_none()
    );

    sim.session.set_binary_frame_for_test(4);
    sim.tick_pending_building_detonation(
        center,
        &rules,
        Some(&registry),
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert!(sim.substrate.entities.get(center).is_some());
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(8, 5).overlay_id,
        Some(0)
    );

    sim.session.set_binary_frame_for_test(5);
    sim.tick_pending_building_detonation(
        center,
        &rules,
        Some(&registry),
        &mut crate::sim::world::FrameEffects::default(),
    );
    // An `Explodes=` barrel's DestructionEffects arms a zero death fuse
    // (`0x00441C43`), so the forced C4 hit leaves it alive at Health 0
    // (`0x0044266B..0x00442690`) until its next Update retires it.
    let expired = sim.substrate.entities.get(center).unwrap();
    assert_eq!(expired.health.current(), 0);
    assert!(!expired.death.is_dying() && expired.lifecycle.object_alive());
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(8, 5).overlay_id,
        None,
        "the barrel DeathWeapon removes the wall in the same expiry transaction"
    );
    sim.object_ai_visit_one(
        center,
        Some(&rules),
        &mut crate::sim::world::ObjectAiCtx {
            overlay_registry: Some(&registry),
            ..Default::default()
        },
    );
    let retired = sim
        .substrate
        .entities
        .get(center)
        .expect("UnInit keeps physical storage until the pending-delete drain");
    assert!(retired.death.is_dying() && !retired.lifecycle.object_alive());
    assert!(!retired.seldom.in_logic_vector.is_member());
    assert!(!sim.substrate.occupancy.contains_entity(8, 5, center));
    assert!(sim.substrate.pending_delete.contains_object(&center));
    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(center).is_none());
}

#[test]
fn gsi_04_07_damage_postmortem_exact_zero_callbacks_precede_restore() {
    use crate::sim::world::LifecycleTestEvent;

    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=SOURCE\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=BARREL\n\
         [Warheads]\n0=DelayWH\n\
         [SOURCE]\nStrength=100\nArmor=heavy\nCost=100\n\
         [BARREL]\nStrength=5\nArmor=concrete\nCost=700\nCanC4=yes\n\
         EligibleForDelayKill=yes\n\
         [DelayWH]\nCellSpread=1\nPercentAtMax=1\nCausesDelayKill=yes\n\
         DelayKillFrames=5\nDelayKillAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("PostMortem callback rules");
    let mut sim = crate::sim::world::Simulation::new();
    let source_id = sim
        .spawn_object("SOURCE", "SourceHouse", 6, 5, 0, &rules)
        .expect("source spawns");
    let target_id = sim
        .spawn_object("BARREL", "VictimHouse", 8, 5, 0, &rules)
        .expect("eligible target spawns");
    let source_owner = sim.substrate.entities.get(source_id).unwrap().owner();
    let victim_owner = sim.substrate.entities.get(target_id).unwrap().owner();
    register_computer_houses(&mut sim.houses, &[source_owner, victim_owner]);
    let arm_source_target = |sim: &mut crate::sim::world::Simulation| {
        let source = sim.substrate.entities.get_mut(source_id).unwrap();
        set_mission(
            source,
            MissionType::Attack,
            MissionId::from_known(MissionType::Guard),
        );
        source
            .attack_target
            .set_for_test(Some(TargetKind::Entity(target_id)));
    };
    arm_source_target(&mut sim);
    sim.substrate
        .entities
        .get_mut(target_id)
        .unwrap()
        .seldom
        .selection
        .set_selected_for_test(true);
    let delay_wh = sim.interner.intern("DelayWH");

    let hit = |sim: &mut crate::sim::world::Simulation| {
        sim.commit_noncombat_aoe_hits(
            &rules,
            None,
            &[EntityDamageEvent::area(
                target_id,
                10,
                0,
                source_id,
                Some(source_owner),
                delay_wh,
            )],
            &mut crate::sim::world::FrameEffects::default(),
        );
    };
    hit(&mut sim);

    let pending = sim
        .substrate
        .entities
        .get(target_id)
        .unwrap()
        .building
        .pending_c4_detonation
        .pending()
        .expect("PostMortem arms the shared timer after callbacks");
    assert_eq!(pending.timer().start_frame(), 0);
    assert_eq!(pending.timer().duration(), 5);
    assert_eq!(pending.source_entity_id(), None);
    let target = sim.substrate.entities.get(target_id).unwrap();
    assert_eq!(target.health.current(), 1);
    assert!(target.lifecycle.object_alive() && !target.lifecycle.in_limbo());
    assert!(target.seldom.in_logic_vector.is_member() && target.lifecycle.cell_marked());
    assert!(
        !target.seldom.selection.is_selected(),
        "ObjectClass::Detach_All(1) deselects before detach"
    );
    assert!(sim.substrate.occupancy.contains_entity(8, 5, target_id));
    assert!(!sim.substrate.pending_delete.contains_object(&target_id));
    assert_eq!(
        target.seldom.kill_record.killer(),
        None,
        "the synchronous callback consumes attribution before HP1 restore"
    );
    let source = sim.substrate.entities.get(source_id).unwrap();
    assert_eq!(source.mission.current().known(), Some(MissionType::Guard));
    assert!(source.attack_target.target().is_none());
    assert_eq!(sim.houses[&victim_owner].stats().buildings_lost(), 1);
    assert_eq!(sim.houses[&source_owner].stats().buildings_killed(), 1);
    assert_eq!(sim.houses[&source_owner].economy().score(), 700);

    let events = sim.lifecycle_test_events_for_test();
    let kill_index = events
        .iter()
        .position(|event| {
            *event
                == (LifecycleTestEvent::RecordKillBookkeeping {
                    stable_id: target_id,
                })
        })
        .expect("kill callback was traced");
    let destroy_index = events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::DestroyNotifyBoundary {
                    stable_id: target_id,
                }
        })
        .expect("Destroy notify boundary was traced");
    let radio_index = events
        .iter()
        .position(|event| {
            *event
                == (LifecycleTestEvent::DestroyRadioBreakCompleted {
                    stable_id: target_id,
                })
        })
        .expect("Building Destroy broadcasts BREAK");
    let deselect_index = events
        .iter()
        .position(|event| {
            *event
                == (LifecycleTestEvent::DestroyDeselected {
                    stable_id: target_id,
                })
        })
        .expect("Object Destroy deselects");
    let source_detach_index = events
        .iter()
        .position(|event| {
            matches!(
                event,
                LifecycleTestEvent::UninitRemovalListenerVisited {
                    expired_id,
                    listener_id,
                    target_alive: true,
                    target_in_limbo: false,
                } if *expired_id == target_id && *listener_id == source_id
            )
        })
        .expect("represented source receives pointer expiry");
    assert!(
        kill_index < radio_index
            && radio_index < deselect_index
            && deselect_index < destroy_index
            && destroy_index < source_detach_index
    );
    assert!(!events.iter().any(|event| {
        matches!(
            event,
            LifecycleTestEvent::UninitClassPre { stable_id }
                | LifecycleTestEvent::UninitAliveCleared { stable_id }
                | LifecycleTestEvent::PendingDeleteQueued { stable_id }
                if *stable_id == target_id
        )
    }));

    // A later equal candidate is longer than the four frames remaining. Native
    // keeps the original timer bytes, but reruns Object's exact-zero callbacks.
    let original_pending = pending;
    sim.session.set_binary_frame_for_test(1);
    arm_source_target(&mut sim);
    sim.clear_lifecycle_test_events_for_test();
    hit(&mut sim);
    let target = sim.substrate.entities.get(target_id).unwrap();
    assert_eq!(target.health.current(), 1);
    assert_eq!(
        target.building.pending_c4_detonation.pending(),
        Some(original_pending)
    );
    assert!(target.lifecycle.object_alive() && target.seldom.in_logic_vector.is_member());
    assert!(sim.substrate.occupancy.contains_entity(8, 5, target_id));
    assert!(!sim.substrate.pending_delete.contains_object(&target_id));
    assert_eq!(sim.houses[&victim_owner].stats().buildings_lost(), 2);
    assert_eq!(sim.houses[&source_owner].stats().buildings_killed(), 2);
    assert_eq!(sim.houses[&source_owner].economy().score(), 1_400);
    assert!(sim.lifecycle_test_events_for_test().iter().any(|event| {
        *event
            == (LifecycleTestEvent::RecordKillBookkeeping {
                stable_id: target_id,
            })
    }));
    assert!(
        sim.substrate
            .entities
            .get(source_id)
            .unwrap()
            .attack_target
            .target()
            .is_none()
    );
}

#[test]
fn gsi_04_07_damage_postmortem_fresh_null_expiry_does_not_recredit_initial_killer() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=SOURCEA\n1=SOURCEB\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=BARREL\n\
         [Warheads]\n0=DelayWH\n1=OrdinaryWH\n2=Super\n\
         [CombatDamage]\nC4Warhead=Super\n\
         [SOURCEA]\nStrength=100\nArmor=heavy\n\
         [SOURCEB]\nStrength=100\nArmor=heavy\n\
         [BARREL]\nStrength=5\nArmor=concrete\nCost=700\nCanC4=yes\n\
         EligibleForDelayKill=yes\n\
         [DelayWH]\nCellSpread=1\nPercentAtMax=1\nCausesDelayKill=yes\n\
         DelayKillFrames=5\nDelayKillAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [OrdinaryWH]\nCellSpread=0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Super]\nCellSpread=0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("fresh PostMortem attribution rules");
    let mut sim = crate::sim::world::Simulation::new();
    sim.resolve_type_handles(&rules);
    let source_a = sim
        .spawn_object("SOURCEA", "HouseA", 4, 5, 0, &rules)
        .expect("initial source spawns");
    let source_b = sim
        .spawn_object("SOURCEB", "HouseB", 5, 5, 0, &rules)
        .expect("later source spawns");
    let expiry_target = sim
        .spawn_object("BARREL", "VictimHouse", 8, 5, 0, &rules)
        .expect("expiry target spawns");
    let later_target = sim
        .spawn_object("BARREL", "VictimHouse", 10, 5, 0, &rules)
        .expect("later ordinary target spawns");
    let owner_a = sim.substrate.entities.get(source_a).unwrap().owner();
    let owner_b = sim.substrate.entities.get(source_b).unwrap().owner();
    let victim_owner = sim.substrate.entities.get(expiry_target).unwrap().owner();
    let owners = [owner_a, owner_b, victim_owner];
    register_computer_houses(&mut sim.houses, &owners);
    let delay_wh = sim.interner.intern("DelayWH");
    let ordinary_wh = sim.interner.intern("OrdinaryWH");

    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[
            EntityDamageEvent::area(expiry_target, 10, 0, source_a, Some(owner_a), delay_wh),
            EntityDamageEvent::area(later_target, 10, 0, source_a, Some(owner_a), delay_wh),
        ],
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(sim.houses[&owner_a].stats().buildings_killed(), 2);
    assert_eq!(sim.houses[&owner_a].economy().score(), 1_400);
    for target_id in [expiry_target, later_target] {
        let target = sim.substrate.entities.get(target_id).unwrap();
        assert_eq!(target.health.current(), 1);
        assert_eq!(target.seldom.kill_record.killer(), None);
    }

    // A different ordinary fatal transaction after restoration must not be
    // blocked by the consumed initial callback attribution.
    sim.session.set_binary_frame_for_test(1);
    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[EntityDamageEvent::area(
            later_target,
            10,
            0,
            source_b,
            Some(owner_b),
            ordinary_wh,
        )],
        &mut crate::sim::world::FrameEffects::default(),
    );
    let later = sim.substrate.entities.get(later_target).unwrap();
    assert_eq!(later.health.current(), 0);
    assert_eq!(later.seldom.kill_record.killer(), Some(owner_b));
    assert_eq!(sim.houses[&owner_b].stats().buildings_killed(), 1);
    assert_eq!(sim.houses[&owner_b].economy().score(), 700);

    let initial_killer_before_expiry = (
        sim.houses[&owner_a].stats().buildings_killed(),
        sim.houses[&owner_a].economy().score(),
    );
    sim.session.set_binary_frame_for_test(5);
    sim.tick_pending_building_detonation(
        expiry_target,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    );
    let expired = sim.substrate.entities.get(expiry_target).unwrap();
    assert_eq!(expired.health.current(), 0);
    assert_eq!(expired.seldom.kill_record.killer(), None);
    assert!(sim.substrate.pending_delete.contains_object(&expiry_target));
    assert_eq!(
        (
            sim.houses[&owner_a].stats().buildings_killed(),
            sim.houses[&owner_a].economy().score(),
        ),
        initial_killer_before_expiry,
        "fresh PostMortem expiry packet is sourceless and cannot recredit HouseA"
    );
}

/// The death arm's gate, then the payload it admits.
fn death_arm_payload(
    rules: &RuleSet,
    obj: &crate::rules::object_type::ObjectType,
    veterancy: u16,
    current_weapon_number: i32,
    current_weapon: Option<&str>,
    interner: &mut StringInterner,
) -> Option<(i32, InternedId, InternedId)> {
    let numbered_weapon =
        super::combat_weapon::weapon_for_index(obj, veterancy, current_weapon_number)
            .and_then(|record| record.weapon_id)
            .and_then(|weapon_id| rules.weapon(weapon_id));
    let current_weapon = current_weapon.and_then(|weapon_id| rules.weapon(weapon_id));
    super::death_arm_explodes(obj, veterancy, numbered_weapon)
        .then(|| super::fire_death_weapon_payload(rules, obj, current_weapon, interner))
        .flatten()
        .map(|payload| (payload.damage, payload.warhead, payload.weapon_ref))
}

#[test]
fn gsi_04_07_damage_death_weapon_gate_selection_and_native_damage() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=FV\n1=NANRCT\n2=SLOTGATE\n3=DEFAULTED\n4=CURRENT\n5=EARLYDEFAULT\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [FV]\nStrength=200\nArmor=light\nPrimary=HoverMissile\nDeathWeapon=CRNuke\n\
         [NANRCT]\nStrength=1000\nArmor=concrete\nExplodes=yes\nDeathWeapon=NukePayload\nDeathWeaponDamageModifier=.5\n\
         [SLOTGATE]\nStrength=100\nArmor=light\nPrimary=Ordinary\nSecondary=SuicideGun\nDeathWeapon=SlotBoom\n\
         [DEFAULTED]\nStrength=601\nArmor=light\nExplodes=yes\n\
         [CURRENT]\nStrength=100\nArmor=light\nExplodes=yes\nPrimary=Ordinary\nDeathWeaponDamageModifier=.5\n\
         [EARLYDEFAULT]\nStrength=1\nSecondary=DefaultDeath\n\
         [HoverMissile]\nDamage=25\nWarhead=OrdinaryWH\n\
         [CRNuke]\nDamage=999\nWarhead=NukeWH\n\
         [NukePayload]\nDamage=600\nWarhead=NukeWH\n\
         [Ordinary]\nDamage=40\nWarhead=OrdinaryWH\n\
         [SuicideGun]\nDamage=40\nWarhead=OrdinaryWH\nSuicide=yes\n\
         [SlotBoom]\nDamage=225\nWarhead=SlotWH\n\
         [DefaultDeath]\nDamage=999\nWarhead=DefaultWH\n\
         [CombatDamage]\nDeathWeapon=DefaultDeath\n\
         [OrdinaryWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [NukeWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [SlotWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [DefaultWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("death-weapon producer rules");
    let mut interner = test_interner();

    assert_eq!(
        death_arm_payload(
            &rules,
            rules.object("FV").unwrap(),
            0,
            0,
            None,
            &mut interner,
        ),
        None,
        "DeathWeapon without Explodes/current Suicide is inert (stock FV shape)"
    );

    let (nanrct_damage, nanrct_wh, nanrct_weapon) = death_arm_payload(
        &rules,
        rules.object("NANRCT").unwrap(),
        0,
        0,
        None,
        &mut interner,
    )
    .unwrap();
    assert_eq!(nanrct_damage, 300, "ftol(600 * 0.5f) must be 300");
    assert_eq!(interner.resolve(nanrct_wh), "NukeWH");
    assert_eq!(interner.resolve(nanrct_weapon), "NukePayload");

    assert_eq!(
        death_arm_payload(
            &rules,
            rules.object("SLOTGATE").unwrap(),
            0,
            0,
            None,
            &mut interner,
        ),
        None,
        "ordinary current Primary does not admit the helper"
    );
    // The gate reads the weapon at CurrentWeaponNumber (`GetWeapon(+0x138)`).
    let (suicide_damage, suicide_wh, suicide_weapon) = death_arm_payload(
        &rules,
        rules.object("SLOTGATE").unwrap(),
        0,
        1,
        Some("Ordinary"),
        &mut interner,
    )
    .unwrap();
    assert_eq!(suicide_damage, 225);
    assert_eq!(interner.resolve(suicide_wh), "SlotWH");
    assert_eq!(interner.resolve(suicide_weapon), "SlotBoom");

    // The payload fires GetCurrentWeapon (vtable `+0x3F4`).
    let (current_damage, current_wh, current_weapon) = death_arm_payload(
        &rules,
        rules.object("CURRENT").unwrap(),
        0,
        0,
        Some("Ordinary"),
        &mut interner,
    )
    .unwrap();
    assert_eq!(current_damage, 20);
    assert_eq!(interner.resolve(current_wh), "OrdinaryWH");
    assert_eq!(interner.resolve(current_weapon), "Ordinary");

    let (default_damage, default_wh, default_weapon) = death_arm_payload(
        &rules,
        rules.object("DEFAULTED").unwrap(),
        0,
        0,
        None,
        &mut interner,
    )
    .unwrap();
    assert_eq!(
        default_damage, 300,
        "Rules fallback is ftol(Strength * 0.5)"
    );
    assert_eq!(interner.resolve(default_wh), "DefaultWH");
    assert_eq!(interner.resolve(default_weapon), "DefaultDeath");
}

#[test]
fn v3_killing_aoe_emits_exactly_one_detonation_anim() {
    // V3 splash kills a low-HP target with no Explosion= list. Only ONE
    // detonation occurred, so ONE AnimList anim starts; the kill adds none.
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=MTNK\n1=WEAK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=V3W\n\n\
         [WEAK]\nStrength=10\nArmor=heavy\nSpeed=6\n\n\
         [V3W]\nDamage=200\nROF=20\nRange=10\nWarhead=V3WH\n\n\
         [Animations]\n0=V3EXP\n\
         [V3WH]\nCellSpread=1\nPercentAtMax=1\nAnimList=V3EXP\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("v3 kill test rules should parse");
    initialize_fixture_anim_types(&mut rules);

    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_entity(2, "WEAK", 8, 5, 10)); // dies in one hit
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    align_attackers_to_targets(&mut store);
    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));

    assert_eq!(
        result.consequences.effects().despawned_ids.len(),
        1,
        "target must die (test setup invariant)"
    );
    assert_eq!(
        result.fixture_anims.len(),
        1,
        "kill must start exactly one anim — no double from the kill handler"
    );
}

#[test]
fn gsi_04_11_death_weapon_anim_precedes_outer_detonation_anim() {
    // A Demo-Truck-style entity (Explodes=yes, primary warhead with its own
    // AnimList) is killed by a tank with a different warhead and AnimList.
    // ReceiveDamage synchronously completes the demo's UCEXPLOD death weapon;
    // only then does the outer Bullet detonation start TANKEXP.
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=TNK\n1=DEMO\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [TNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=TANKW\n\n\
         [DEMO]\nStrength=100\nArmor=light\nSpeed=6\nPrimary=DEMOW\nExplodes=yes\n\n\
         [TANKW]\nDamage=100\nROF=20\nRange=10\nWarhead=TANKHIT\n\n\
         [DEMOW]\nDamage=200\nROF=50\nRange=4\nWarhead=DEMOWH\n\n\
         [TANKHIT]\nCellSpread=0\nPercentAtMax=1\nAnimList=TANKEXP\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\n\
         [Animations]\n0=TANKEXP\n1=UCEXPLOD\n\
         [DEMOWH]\nCellSpread=2\nPercentAtMax=0.5\nAnimList=UCEXPLOD\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("demo-truck test rules should parse");
    initialize_fixture_anim_types(&mut rules);

    let mut store = EntityStore::new();
    store.insert(make_entity(1, "TNK", 5, 5, 300));
    store.insert(make_entity(2, "DEMO", 8, 5, 100));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    align_attackers_to_targets(&mut store);
    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));

    let tankexp = interner.intern("TANKEXP");
    let ucexplod = interner.intern("UCEXPLOD");
    assert_eq!(
        result
            .fixture_anims
            .iter()
            .map(|effect| effect.type_id)
            .collect::<Vec<_>>(),
        vec![ucexplod, tankexp]
    );
}

/// Fire_Death_Weapon's bullet takes DetonateAtCoord's tail (`0x00469AA4`)
/// inside the dying object's ReceiveDamage: an Inviso death weapon scatters
/// its anim by one raw Scenario draw, ahead of the outer shot's anim, and
/// draws no cluster successor (`0x0070D782` calls DetonateAtCoord directly).
// Phobos `ROF.RandomDelay` (`0x006FD0B5`) redraws the full-rearm jitter, so
// this stock golden only holds without the capability.
#[cfg(not(feature = "phobos"))]
#[test]
fn an_inviso_death_weapon_scatters_its_anim_inside_the_receiver() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=TNK\n1=DEMO\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [TNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=TANKW\n\n\
         [DEMO]\nStrength=100\nArmor=light\nSpeed=6\nPrimary=DEMOW\nExplodes=yes\n\n\
         [TANKW]\nDamage=100\nROF=20\nRange=10\nWarhead=TANKHIT\n\n\
         [DEMOW]\nDamage=200\nROF=50\nRange=4\nProjectile=DEMOP\nWarhead=DEMOWH\n\n\
         [DEMOP]\nInviso=yes\n\n\
         [TANKHIT]\nCellSpread=0\nPercentAtMax=1\nAnimList=TANKEXP\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\n\
         [Animations]\n0=TANKEXP\n1=UCEXPLOD\n\
         [DEMOWH]\nCellSpread=2\nPercentAtMax=0.5\nAnimList=UCEXPLOD\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("Inviso demo rules should parse");
    initialize_fixture_anim_types(&mut rules);

    let mut store = EntityStore::new();
    store.insert(make_entity(1, "TNK", 5, 5, 300));
    store.insert(make_entity(2, "DEMO", 8, 5, 100));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    let mut scenario_rng = SimRng::new(1);
    let mut expected_rng = scenario_rng.clone();
    // FireAt's GetROF jitter, then the death weapon's scatter around its
    // GetCoords. The tank's shot names no BulletType: no scatter, no cluster.
    expected_rng.next_range_u32_inclusive(0, 2);
    let demo = (8, 5, SimFixed::from_num(128), SimFixed::from_num(128));
    let death_anim = inviso_scatter::scatter_inviso_effect_coord(
        crate::map::cell_index::CellGrid::Retail,
        &mut expected_rng,
        demo.0,
        demo.1,
        demo.2,
        demo.3,
    );
    align_attackers_to_targets(&mut store);
    let result = tick(&mut store, &rules, &mut interner, 0, &mut scenario_rng);

    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
    assert_eq!(store.get(2).unwrap().health.current(), 0);
    let ucexplod = interner.intern("UCEXPLOD");
    let tankexp = interner.intern("TANKEXP");
    assert_eq!(
        result
            .fixture_anims
            .iter()
            .map(|anim| anim.type_id)
            .collect::<Vec<_>>(),
        vec![ucexplod, tankexp]
    );
    assert_eq!(constructed_anim_coord(&result.fixture_anims[0]), death_anim);
    assert_ne!(death_anim, demo, "seed 1 moves the anim");
}

/// A death weapon's bullet is never placed, so the anim tail picks its anim
/// by the land at the bullet's zero Location (`0x00469B0E`/`0x00469BA2`; the
/// constructor's `0x00AC1380` copy), cell (0,0). Over water a Conventional
/// death weapon still takes its AnimList anim; the shot that killed it, read
/// at its impact, takes the SplashList one.
#[test]
fn a_death_weapon_over_water_picks_its_anim_at_the_zero_location() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=TNK\n1=DEMO\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [CombatDamage]\nSplashList=SPLASH\n\n\
         [TNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=TANKW\n\n\
         [DEMO]\nStrength=100\nArmor=light\nSpeed=6\nPrimary=DEMOW\nExplodes=yes\n\n\
         [TANKW]\nDamage=100\nROF=20\nRange=10\nWarhead=TANKHIT\n\n\
         [DEMOW]\nDamage=200\nROF=50\nRange=4\nWarhead=DEMOWH\n\n\
         [TANKHIT]\nCellSpread=0\nConventional=yes\nAnimList=TANKEXP\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\n\
         [Animations]\n0=TANKEXP\n1=UCEXPLOD\n2=SPLASH\n\
         [DEMOWH]\nCellSpread=0\nConventional=yes\nAnimList=UCEXPLOD\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("death weapon rules should parse");
    initialize_fixture_anim_types(&mut rules);

    let mut store = EntityStore::new();
    for mut entity in [
        make_entity(1, "TNK", 5, 5, 300),
        make_entity(2, "DEMO", 8, 5, 100),
    ] {
        entity.lifecycle.set_cell_marked_for_test(true);
        store.insert(entity);
    }
    let mut occupancy = OccupancyGrid::rebuild(&store);
    let mut interner = test_interner();
    let mut terrain = crate::sim::tiberium::test_support::flat_terrain(16, 16);
    let water = terrain.cell_mut_for_test(8, 5).unwrap();
    water.land_type = 2;
    water.base_land_type = 2;
    // The tank's shot lands on the demo truck and kills it.
    let shot = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 7,
        source_id: 1,
        target: crate::sim::projectile::ProjectileTarget::Entity(2),
        impact: crate::sim::projectile::ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
        payload: payload_from_weapon(
            100,
            interner.intern("TANKHIT"),
            interner.intern("TANKW"),
            &rules,
            &mut interner,
        ),
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    let result = tick_combat_with_fog_and_main_rng(
        &mut store,
        &mut occupancy,
        &rules,
        &mut interner,
        Some(handles),
        None,
        &BTreeMap::new(),
        &mut crate::sim::houses::Houses::default(),
        &crate::map::houses::HouseAllianceMap::default(),
        None,
        None,
        None,
        Some(&mut terrain),
        1,
        100,
        1,
        &[1, 2],
        &[shot],
        None,
        &mut SimRng::new(1),
        &mut SimRng::new(1),
        None,
    );

    assert_eq!(store.get(2).unwrap().health.current(), 0);
    let anims: Vec<_> = result
        .fixture_anims
        .iter()
        .map(|anim| interner.resolve(anim.type_id).to_string())
        .collect();
    assert_eq!(anims, ["UCEXPLOD", "SPLASH"]);
}

/// Fire_Death_Weapon detonates at its object's GetCoords (vt+0x48 at
/// `0x0070D77C`): for a building that is the foundation centre
/// (`BuildingClass::GetCoords @ 0x00447AC0`), not its Location's top-left
/// cell. A victim two cells east of a 3x3 centre is 512 leptons from the
/// blast; from the Location it would be about 809, outside CellSpread 2.5.
#[test]
fn a_buildings_death_weapon_detonates_at_its_foundation_centre() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=VICTIM\n[AircraftTypes]\n\
         [BuildingTypes]\n0=DERRICK\n\
         [Warheads]\n0=KillWH\n1=OilWH\n\
         [DERRICK]\nStrength=100\nArmor=wood\nExplodes=yes\nDeathWeapon=OilBoom\n\
         [VICTIM]\nStrength=1000\nArmor=heavy\n\
         [OilBoom]\nDamage=100\nWarhead=OilWH\n\
         [KillWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [OilWH]\nCellSpread=2.5\nPercentAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("derrick rules");
    let mut sim = Simulation::with_seed(1);
    let owner = sim.interner.intern("Neutral");
    let mut derrick = GameEntity::test_default_of_category(
        1,
        "DERRICK",
        "Neutral",
        10,
        10,
        EntityCategory::Structure,
    );
    derrick.set_owner_for_test(owner);
    derrick.set_type_ref_for_test(sim.interner.intern("DERRICK"));
    derrick
        .seldom
        .type_facts
        .set_foundation_for_test("3x3".to_string().into());
    derrick.set_is_voxel_for_test(false);
    derrick.health.set_for_test(100);
    sim.substrate.entities.insert(derrick);
    let _ = sim.reveal(1);
    let mut victim = GameEntity::test_default(2, "VICTIM", "Neutral", 13, 11);
    victim.set_owner_for_test(owner);
    victim.set_type_ref_for_test(sim.interner.intern("VICTIM"));
    victim.health.set_for_test(1000);
    sim.substrate.entities.insert(victim);
    let _ = sim.reveal(2);
    sim.set_next_stable_object_id_for_test(3);

    let kill_wh = sim.interner.intern("KillWH");
    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[EntityDamageEvent::area(
            1,
            100,
            0,
            RAD_NO_ATTACKER,
            None,
            kill_wh,
        )],
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(sim.substrate.entities.get(1).unwrap().health.current(), 0);
    assert_eq!(
        sim.substrate.entities.get(2).unwrap().health.current(),
        900,
        "the full 100 at 512 leptons from the foundation centre"
    );
}

/// A destroyed vehicle with `MaxDebris=` alone scatters SHP chunks from
/// `[General] MetallicDebris=`.
///
/// gamemd-derived: `TechnoClass::ReceiveDamage @ 0x00701900`. With
/// `DebrisAnims.Count == 0` (`0x007023FC`) and `DebrisTypes.Count == 0`
/// (`0x007024D2`) the whole budget goes to the arm at `0x007024E0`, one
/// `RandomRanged(0, RulesClass+0x14C - 1)` per chunk at `0x0070253A`. 155 stock
/// TechnoTypes take exactly this path in gamemd — `MTNK` (Grizzly), `ZEP` (Kirov
/// Airship), `NAPSYA` (Psychic Amplifier), and 11 of the 12 `[AircraftTypes]`;
/// the twelfth, `APACHE`, has no `[APACHE]` section in `rulesmd.ini` at all, so
/// it keeps the `TechnoTypeClass` constructor default of 0 (`0x00710B00` ->
/// `0x00710FB7`) and takes no draw. Before this landed, none of them threw
/// anything.
///
/// 155, not 168: `ObjectType::from_ini_section` reads `MaxDebris=` case-exactly,
/// the way gamemd's `INIClass` does — it CRCs the raw key bytes and folds no
/// case anywhere on the path. 17 stock `[VehicleTypes]` spell the key
/// `Maxdebris=` — 14 buildable (Rhino, Apocalypse, Lasher, Tesla, Gattling,
/// Prism, V3, Magnetron, Master Mind, Battle Fortress, IFV, Flak Track, Landing
/// Craft, Armored Transport) and 3 at `TechLevel=-1` (`CMON`, `HORV`, `UTNK`) —
/// and every one of them takes the constructor default 0 and throws no debris
/// at all. 13 of the 17 would have sat on this metallic arm, which is exactly
/// the 168 - 155 gap; the other 4 (`CMON`, `FV`, `HORV`, `HTK`) author
/// `DebrisTypes=TIRE` and would have sat on the voxel arm. All three exemplars
/// named above spell `MaxDebris` exactly, so they are unaffected either way.
#[test]
fn gsi_05_14_a_dying_vehicle_scatters_metallic_debris() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nMetallicDebris=DBRIS1LG,DBRIS2LG,DBRIS3LG\n\
         [VehicleTypes]\n0=MTNK\n1=HTNK\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\n\
         [HTNK]\nStrength=1\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\nMaxDebris=4\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("metallic-debris fixture parses");

    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet"));
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    align_attackers_to_targets(&mut store);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(9));

    let names: Vec<&str> = result
        .consequences
        .effects()
        .explosion_effects
        .iter()
        .map(|effect| interner.resolve(effect.shp_name))
        .collect();
    assert!(
        names.iter().any(|name| name.starts_with("DBRIS")),
        "the death should scatter MetallicDebris chunks, got {names:?}"
    );
    assert!(
        result.consequences.effects().voxel_debris.is_empty(),
        "a type with no DebrisTypes= throws no VoxelAnims"
    );
}

/// `DebrisAnims=` on the section wins over `[General] MetallicDebris=`.
///
/// gamemd-derived: `0x007023EF` reads `TechnoType+0x5D4` first and only a zero
/// count falls through to the Rules list at `0x007024BB`. All 166 stock
/// `DebrisAnims=` lines sit on `[BuildingTypes]` sections, so only a structure
/// ever takes this arm — but not every structure does: of the 292 registered
/// `[BuildingTypes]` that throw, 126 carry `MaxDebris=` alone and fall through
/// to `[General] MetallicDebris=`. All three figures are basis-independent: the
/// 17 sections that spell the key `Maxdebris=` are every one of them a
/// `[VehicleTypes]`, so the case-exact read moves no building count.
#[test]
fn gsi_05_14_a_dying_building_uses_its_own_debris_anims() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nMetallicDebris=DBRIS1LG,DBRIS2LG,DBRIS3LG\n\
         [VehicleTypes]\n0=MTNK\n1=HTNK\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\n\
         [HTNK]\nStrength=1\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\n\
         MinDebris=3\nMaxDebris=4\nDebrisAnims=DBRI-WM1,DBRI-WM2\n\
         [Animations]\n0=DBRI-WM1\n1=DBRI-WM2\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("debris-anims fixture parses");

    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet"));
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    align_attackers_to_targets(&mut store);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(21));

    let names: Vec<&str> = result
        .consequences
        .effects()
        .explosion_effects
        .iter()
        .map(|effect| interner.resolve(effect.shp_name))
        .collect();
    let own: usize = names.iter().filter(|n| n.starts_with("DBRI-WM")).count();
    // `MinDebris=3`, `MaxDebris=4` pins the budget at 3 with no draw.
    assert_eq!(
        own, 3,
        "the whole budget comes from DebrisAnims=: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.starts_with("DBRIS")),
        "the Rules MetallicDebris arm must not also fire: {names:?}"
    );
}

/// A dying building throws its debris from its GetCoords (`vtable+0x48` at
/// `0x007024FC`), which for a building is the foundation centre
/// (`BuildingClass::GetCoords @ 0x00447AC0`: Location + (W*128 - 128,
/// H*128 - 128)), 20 leptons up — not from its top-left cell.
#[test]
fn a_dying_buildings_debris_leaves_from_its_centre() {
    let ini = IniFile::from_str(
        "[General]\nMetallicDebris=DBRIS1LG\n\
         [VehicleTypes]\n0=MTNK\n\
         [BuildingTypes]\n0=BIG\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\n\
         [BIG]\nStrength=1\nArmor=heavy\nMinDebris=1\nMaxDebris=2\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let art = IniFile::from_str("[BIG]\nFoundation=3x3\n");
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art)
        .expect("building debris fixture parses");

    let mut store = EntityStore::new();
    let _ = test_intern("BIG");
    let mut building = make_structure_entity(2, "BIG", 8, 5, 0, 1);
    // Construction stamps the type's `Foundation=3x3`.
    building
        .seldom
        .type_facts
        .set_foundation_for_test("3x3".to_string().into());
    let location = (
        8 * 256 + building.position.sub_x().to_num::<i32>(),
        5 * 256 + building.position.sub_y().to_num::<i32>(),
    );
    store.insert(building);
    let mut interner = test_interner();

    let effects = run_combat_death_handoff(&mut store, &rules, &mut interner, &[2], &[]);

    let debris = interner.intern("DBRIS1LG");
    let spawns: Vec<_> = effects
        .explosion_effects
        .iter()
        .filter(|effect| effect.shp_name == debris)
        .map(|effect| effect.death.expect("an exact construction").coord)
        .collect();
    assert_eq!(spawns.len(), 1, "MinDebris=1, MaxDebris=2 pins one piece");
    assert_eq!(
        (spawns[0].x, spawns[0].y),
        (location.0 + 256, location.1 + 256),
        "one cell in from the top-left for a 3x3"
    );
}

/// A destroyed harvester throws `[VoxelAnims]` tyres, and its budget never
/// reaches the SHP arms.
///
/// gamemd-derived: with `DebrisTypes.Count > 0` the loop at `0x007022FE`
/// drains the budget to zero before either SHP arm is tested, so a type that
/// names `DebrisTypes=` throws VXL debris and nothing else. All 36 stock
/// `DebrisTypes=` lines name `TIRE`, and 32 of the 36 reach the block in gamemd
/// — `CMON`, `FV`, `HORV` and `HTK` spell `Maxdebris=` and so get the default 0.
/// `[HARV]` spells `MaxDebris=6` exactly, with `DebrisMaximums=4`.
#[test]
fn gsi_05_14_a_dying_harvester_throws_voxel_tires_and_no_shp_debris() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nMetallicDebris=DBRIS1LG,DBRIS2LG,DBRIS3LG\n\
         [VoxelAnims]\n1=TIRE\n\
         [TIRE]\nElasticity=0.8\nMinAngularVelocity=12.0\nMaxAngularVelocity=24.0\n\
         MinZVel=28.0\nMaxZVel=32.0\nMaxXYVel=10.0\nDuration=150\n\
         [VehicleTypes]\n0=MTNK\n1=HTNK\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\n\
         [HTNK]\nStrength=1\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\n\
         MinDebris=5\nMaxDebris=6\nDebrisTypes=TIRE\nDebrisMaximums=4\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("voxel-debris fixture parses");

    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet"));
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    align_attackers_to_targets(&mut store);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(5));

    // MinDebris == MaxDebris - 1 pins the budget at 5 with no draw, and the
    // loop drains exactly that many.
    assert_eq!(result.consequences.effects().voxel_debris.len(), 5);
    let names: Vec<&str> = result
        .consequences
        .effects()
        .explosion_effects
        .iter()
        .map(|effect| interner.resolve(effect.shp_name))
        .collect();
    assert!(
        !names.iter().any(|n| n.starts_with("DBRI")),
        "the voxel loop spends the whole budget: {names:?}"
    );
    // ObjectUnlimbo7494E9 retains the wreck Location; only Bounce gets
    // the constructor's ten-lepton lift74950C before its first AI visit.
    for piece in &result.consequences.effects().voxel_debris {
        assert_eq!(piece.object.duration(), 150);
        assert_eq!(piece.object.world_coord().z, 0);
        assert_eq!(piece.object.bounce().position_leptons().z, 10);
    }
}

/// A type with `MaxDebris=0` costs the shared stream nothing.
///
/// `TEST ECX,ECX / JLE 0x00702572` at `0x00702291` skips the whole block above
/// the budget draw. No `[InfantryTypes]` section authors `MaxDebris=` at all —
/// 0 of the 65 — so this is the common case, and consuming a draw here would
/// shift the cursor for every later consumer in the tick.
#[test]
fn gsi_05_14_a_type_without_maxdebris_takes_no_draw() {
    let ini = "[General]\nMetallicDebris=DBRIS1LG,DBRIS2LG,DBRIS3LG\n\
         [VehicleTypes]\n0=MTNK\n1=HTNK\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\n\
         [HTNK]\nStrength=1\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";
    let rules = RuleSet::from_ini(&IniFile::from_str(ini)).expect("no-debris fixture parses");

    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet"));
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    align_attackers_to_targets(&mut store);

    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(64));
    assert!(result.consequences.effects().voxel_debris.is_empty());
    let names: Vec<&str> = result
        .consequences
        .effects()
        .explosion_effects
        .iter()
        .map(|effect| interner.resolve(effect.shp_name))
        .collect();
    assert!(
        !names.iter().any(|n| n.starts_with("DBRI")),
        "no MaxDebris= means no debris at all: {names:?}"
    );
}
