//! Stock human Guard Aegis acquisition, starting with legal naval Unlimbo
//! and height-aware Kirov Unlimbo/AirTracker registration.

use super::{FrameEffects, Simulation, TickLane};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::line_of_fire::LineOfFireInputs;
use crate::sim::combat::{ScanMission, TargetKind};
use crate::sim::mission::{MissionId, MissionType};
use crate::util::save_codec;

fn eligible_air_target(sim: &Simulation, rules: &RuleSet, aegis: u64) -> Option<u64> {
    crate::sim::combat::combat_targeting::greatest_threat_for_entity(
        &sim.substrate.entities,
        sim.occupancy(),
        rules,
        &sim.interner,
        aegis,
        Some(&sim.fog),
        sim.resolved_terrain.as_ref(),
        true,
        ScanMission::Guard,
        sim.zone_grid.as_ref(),
        LineOfFireInputs {
            overlay_grid: sim.overlay_grid.as_ref(),
            overlay_registry: None,
            alliances: Some(&sim.house_alliances),
        },
        Some(sim),
        None,
    )
    .target()
}

struct GuardScene {
    sim: Simulation,
    rules: RuleSet,
    aegis: u64,
    targets: Vec<u64>,
}

impl GuardScene {
    fn new(elite: bool) -> Option<Self> {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return None;
        };
        let rules = retail.rules;
        let aegis_type = rules.object("AEGIS").unwrap();
        assert!(aegis_type.distributed_fire);
        assert!(aegis_type.can_passive_acquire);
        assert_eq!(aegis_type.primary(), Some("Medusa"));
        for name in ["Medusa", "MedusaE"] {
            assert!(rules.weapon(name).unwrap().omni_fire);
        }
        assert!(rules.object("ZEP").unwrap().considered_aircraft);
        assert!(rules.object("ZEP").unwrap().balloon_hover);

        let mut sim = crate::sim::arena_fixture::rival_water_arena(&rules, 1284, 64);
        // The arena supplies terrain/navigation; establish this visibility
        // domain before Unlimbo can create source and border viewer planes.
        sim.fog = crate::sim::vision::FogState::new(64, 64);
        sim.session
            .set_current_house_for_test(Some(sim.interner.intern("Americans")));
        let aegis = sim
            .spawn_object("AEGIS", "Americans", 24, 24, 0, &rules)
            .expect("retail Aegis Unlimbo on Water");
        let mut targets = Vec::new();
        for (x, y) in [(30, 24), (31, 25)] {
            let id = sim
                .spawn_object_at_height("ZEP", "Russians", x, y, 0, 6, &rules)
                .expect("retail Kirov high Unlimbo");
            let actor = sim.substrate.entities.get(id).unwrap();
            assert!(!actor.lifecycle.in_limbo());
            assert!(actor.lifecycle.cell_marked());
            assert!(actor.in_playfield());
            assert_eq!(actor.air_tracker_cell(), (x as i16, y as i16));
            assert!(actor.air_spatial_bucket().is_some());
            assert_eq!(
                crate::sim::occupancy::cell_list_layer_for_entity(
                    actor,
                    sim.resolved_terrain.as_ref()
                ),
                None,
                "the current Object layer is Air, independently of tracker membership"
            );
            targets.push(id);
        }
        assert_eq!(sim.substrate.entities.airborne().count(), 2);
        let source = sim.substrate.entities.get(aegis).unwrap();
        assert!(sim.houses.get(&source.owner()).unwrap().is_human());
        assert_eq!(
            source.mission.current(),
            MissionId::from_known(MissionType::Guard)
        );
        assert_eq!(source.attack_target.target(), None);
        assert!(!source.passive_scan.acquired());
        assert_eq!(source.passive_scan.last_scan_frame(), 0);
        // Supplied local visibility is map/session knowledge, not an installed
        // combat target or a replacement for the real scanner's eligibility gates.
        let owner = source.owner();
        for &id in &targets {
            let target = sim.substrate.entities.get(id).unwrap();
            sim.fog
                .mark_visible_for_owner(owner, target.position.rx(), target.position.ry());
        }
        let candidate = eligible_air_target(&sim, &rules, aegis)
            .expect("stock acquisition has a legal airborne candidate before frames run");
        assert!(targets.contains(&candidate));

        if elite {
            sim.substrate
                .entities
                .get_mut(aegis)
                .unwrap()
                .veterancy
                .set_elite();
        }
        Some(GuardScene {
            sim,
            rules,
            aegis,
            targets,
        })
    }

    fn advance(&mut self) -> super::SimFrameOutput {
        let commands = self.sim.take_due_commands();
        let output = self
            .sim
            .advance_app_frame(
                &commands,
                Some(&self.rules),
                None,
                33,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("ordinary Aegis frame");
        assert!(output.tick.frame_committed);
        output
    }

    fn next_shot(&mut self) -> super::SimFireEvent {
        for _ in 0..180 {
            let output = self.advance();
            if let Some(shot) = output
                .fire_events
                .into_iter()
                .find(|shot| shot.attacker_id == self.aegis)
            {
                return shot;
            }
        }
        panic!("stock Aegis must fire within its ordinary targeting/Guard cadence");
    }
}

/// Successful Guard FireAt records its argument and performs the real class
/// NULL setter. Next scans exclude fired targets until all have been visited.
#[test]
fn distributed_guard_normal_and_elite_cycle_live_targets_and_reset_only_burst() {
    for elite in [false, true] {
        let Some(mut scene) = GuardScene::new(elite) else {
            return;
        };
        let mut fired = Vec::new();
        for index in 0..3 {
            let shot = scene.next_shot();
            assert_eq!(
                scene.sim.interner.resolve(shot.weapon_id),
                if elite { "MedusaE" } else { "Medusa" }
            );
            assert!(matches!(shot.target, TargetKind::Entity(id) if scene.targets.contains(&id)));
            fired.push(shot.target);
            let source = scene.sim.substrate.entities.get(scene.aegis).unwrap();
            assert_eq!(source.mission.current().known(), Some(MissionType::Guard));
            assert_eq!(source.attack_target.target(), None);
            assert!(
                !source.passive_scan.acquired(),
                "class NULL assignment clears passive acquisition"
            );
            assert_eq!(
                source.weapon_burst.index(),
                0,
                "changed NULL target resets elite Burst2 too"
            );
            assert_eq!(
                source.weapon_burst.radial_index(),
                index + 1,
                "radial launch state is independent"
            );
            assert_eq!(
                source.distributed_fire_history(),
                if index == 1 {
                    vec![Some(fired[0]), Some(fired[1])]
                } else {
                    vec![Some(shot.target)]
                }
            );
            assert_eq!(source.distributed_fire_candidates().len(), 2);
            let bullet = scene
                .sim
                .projectiles
                .iter()
                .map(|(_, bullet)| bullet)
                .find(|bullet| {
                    bullet.source_id() == scene.aegis
                        && bullet.target() == shot.target.projectile_target(&scene.sim)
                })
                .expect("real returned Medusa remains targeted independently of shooter TarCom");
            assert_eq!(bullet.payload().installed_weapon(), Some(shot.weapon_id));
        }
        assert_ne!(
            fired[0], fired[1],
            "both legal Kirovs receive a shot before reuse"
        );
        assert!(scene.targets.iter().all(|id| {
            scene
                .sim
                .substrate
                .entities
                .get(*id)
                .is_some_and(|entity| entity.health.current() > 0)
        }));
    }
}

/// The Unit Attack(1) exception is deliberately different from Guard: an
/// explicit player's target and elite burst survive the same FireAt tail.
#[test]
fn distributed_explicit_attack_retains_target_and_elite_burst() {
    for elite in [false, true] {
        let Some(mut scene) = GuardScene::new(elite) else {
            return;
        };
        let target = TargetKind::Entity(scene.targets[0]);
        scene
            .sim
            .mission_assign_exact(scene.aegis, MissionId::from_known(MissionType::Attack), 0)
            .unwrap();
        scene
            .sim
            .assign_target_represented(
                scene.aegis,
                Some(target),
                Some(&scene.rules),
                None,
                FrameEffects::default(),
            )
            .unwrap();
        for index in 0..2 {
            let shot = scene.next_shot();
            assert_eq!(shot.target, target);
            let source = scene.sim.substrate.entities.get(scene.aegis).unwrap();
            assert_eq!(source.attack_target.target(), Some(target));
            assert!(source.distributed_fire_history().is_empty());
            assert_eq!(
                source.weapon_burst.index(),
                if elite { (index + 1) % 2 } else { 0 }
            );
            assert_eq!(source.weapon_burst.radial_index(), index + 1);
        }
    }
}

/// List-only references must keep the shooter subscribed after its real
/// FireAt NULL setter. Expiry removes one paired entry and one history entry
/// on control0 and1, without TarCom's short-rearm RNG when TarCom is null.
#[test]
fn distributed_list_only_expiry_uses_common_broadcast_and_first_removal() {
    use crate::sim::game_entity::DistributedFireCandidate;
    let Some(mut scene) = GuardScene::new(false) else {
        return;
    };
    let shot = scene.next_shot();
    let TargetKind::Entity(target) = shot.target else {
        panic!("air target");
    };
    let other = *scene.targets.iter().find(|id| **id != target).unwrap();
    let source = scene.sim.substrate.entities.get_mut(scene.aegis).unwrap();
    source.replace_distributed_fire_candidates(vec![
        DistributedFireCandidate {
            target: Some(shot.target),
            score: 17,
        },
        DistributedFireCandidate {
            target: Some(TargetKind::Entity(other)),
            score: 23,
        },
        DistributedFireCandidate {
            target: Some(shot.target),
            score: 29,
        },
    ]);
    source.remember_distributed_fire_target(shot.target);
    let timer = source.passive_scan.timer();
    let rng = scene.sim.scenario_rng.state();
    let main = scene.sim.main_rng.state();
    let mapgen = scene.sim.mapgen_rng.state();
    // Refreshing the existing derived expiry index occurs inside this owner.
    scene
        .sim
        .detach_all_pointer_expired(target, &scene.rules, None, FrameEffects::default());
    let source = scene.sim.substrate.entities.get(scene.aegis).unwrap();
    assert_eq!(
        source
            .distributed_fire_candidates()
            .iter()
            .map(|entry| entry.score)
            .collect::<Vec<_>>(),
        [23, 29]
    );
    assert_eq!(source.distributed_fire_history(), [Some(shot.target)]);
    scene
        .sim
        .notify_pointer_expired(target, super::UninitContext::new(Some(&scene.rules), None));
    let source = scene.sim.substrate.entities.get(scene.aegis).unwrap();
    assert_eq!(
        source.distributed_fire_candidates(),
        [DistributedFireCandidate {
            target: Some(TargetKind::Entity(other)),
            score: 23
        }]
    );
    assert!(source.distributed_fire_history().is_empty());
    assert_eq!(source.passive_scan.timer(), timer);
    assert_eq!(source.attack_target.target(), None);
    assert_eq!(scene.sim.scenario_rng.state(), rng);
    assert_eq!(scene.sim.main_rng.state(), main);
    assert_eq!(scene.sim.mapgen_rng.state(), mapgen);
    scene
        .sim
        .detach_all_pointer_expired(target, &scene.rules, None, FrameEffects::default());
    assert_eq!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.aegis)
            .unwrap()
            .distributed_fire_candidates()
            .len(),
        1
    );
}

#[test]
fn distributed_runtime_serde_clone_and_hash_preserve_order_scores_and_history() {
    use crate::sim::game_entity::{DistributedFireCandidate, GameEntity};
    let Some(mut scene) = GuardScene::new(false) else {
        return;
    };
    let entries = vec![
        DistributedFireCandidate {
            target: Some(TargetKind::Entity(scene.targets[0])),
            score: 3,
        },
        DistributedFireCandidate {
            target: Some(TargetKind::Entity(scene.targets[1])),
            score: 3,
        },
    ];
    let source = scene.sim.substrate.entities.get_mut(scene.aegis).unwrap();
    source.replace_distributed_fire_candidates(entries.clone());
    source.remember_distributed_fire_target(TargetKind::Entity(scene.targets[0]));
    let bytes = save_codec::serialize(source).unwrap();
    let restored: GameEntity = save_codec::deserialize(&bytes).unwrap();
    assert_eq!(save_codec::serialize(&restored).unwrap(), bytes);
    assert_eq!(save_codec::serialize(&source.clone()).unwrap(), bytes);
    let hash = scene.sim.state_hash();
    for changed in [
        vec![entries[1], entries[0]],
        vec![
            DistributedFireCandidate {
                score: 4,
                ..entries[0]
            },
            entries[1],
        ],
    ] {
        scene
            .sim
            .substrate
            .entities
            .get_mut(scene.aegis)
            .unwrap()
            .replace_distributed_fire_candidates(changed);
        assert_ne!(scene.sim.state_hash(), hash);
    }
    scene
        .sim
        .substrate
        .entities
        .get_mut(scene.aegis)
        .unwrap()
        .replace_distributed_fire_candidates(entries);
    assert_eq!(scene.sim.state_hash(), hash);
    scene
        .sim
        .substrate
        .entities
        .get_mut(scene.aegis)
        .unwrap()
        .remember_distributed_fire_target(TargetKind::Entity(scene.targets[1]));
    assert_ne!(scene.sim.state_hash(), hash);
}

/// Normal concrete native Load constructs empty vectors after raw load, for
/// flagfalse types too. Discarded list IDs are not saved strong references;
/// the real TarCom still is. This is the modeled load boundary, not a claim
/// that application saved-game/campaign holds have been lifted.
#[test]
fn distributed_normal_load_clears_all_classes_before_strong_reference_validation() {
    use crate::sim::game_entity::DistributedFireCandidate;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 1284, 64);
    let mut actors = Vec::new();
    for (name, cell) in [
        ("MTNK", (10, 12)),
        ("E1", (15, 12)),
        ("GAPOWR", (20, 12)),
        ("ORCA", (25, 12)),
    ] {
        let id = sim
            .spawn_object(name, "Americans", cell.0, cell.1, 0, &rules)
            .expect("real concrete class admission");
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.replace_distributed_fire_candidates(vec![DistributedFireCandidate {
            target: Some(TargetKind::Entity(u64::MAX)),
            score: 7,
        }]);
        entity.remember_distributed_fire_target(TargetKind::Entity(u64::MAX));
        actors.push(id);
    }
    // Retain a real link independently of the discarded three vectors.
    sim.assign_target_represented(
        actors[0],
        Some(TargetKind::Entity(actors[1])),
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "distributed", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    assert_eq!(
        loaded
            .substrate
            .entities
            .get(actors[0])
            .unwrap()
            .distributed_fire_history()
            .len(),
        1,
        "mechanical transport retains runtime lists"
    );
    loaded
        .restore_after_snapshot_load()
        .expect("stale list-only IDs are discarded, not swizzled");
    for id in &actors {
        let entity = loaded.substrate.entities.get(*id).unwrap();
        assert!(entity.distributed_fire_candidates().is_empty());
        assert!(entity.distributed_fire_history().is_empty());
    }
    assert_eq!(
        loaded
            .substrate
            .entities
            .get(actors[0])
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Entity(actors[1]))
    );
    loaded
        .substrate
        .entities
        .get_mut(actors[0])
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(u64::MAX)));
    loaded
        .substrate
        .entities
        .get_mut(actors[0])
        .unwrap()
        .remember_distributed_fire_target(TargetKind::Entity(u64::MAX));
    assert!(
        loaded.restore_after_snapshot_load().is_err(),
        "a matching discarded list never weakens real TarCom validation"
    );
}

/// Collection consumes the maintained tracker buckets and their insertion
/// order, even between Location movement and the later tracker callback.
#[test]
fn distributed_scan_preserves_retained_bucket_order_across_reentry_and_movement() {
    let Some(mut scene) = GuardScene::new(false) else {
        return;
    };
    let scan = |scene: &mut GuardScene| {
        scene.sim.greatest_threat_represented(
            &scene.rules,
            None,
            scene.aegis,
            ScanMission::Guard,
            None,
        );
        scene
            .sim
            .substrate
            .entities
            .get(scene.aegis)
            .unwrap()
            .distributed_fire_candidates()
            .iter()
            .map(|entry| entry.target.unwrap())
            .collect::<Vec<_>>()
    };
    let [a, b] = [scene.targets[0], scene.targets[1]];
    assert_eq!(
        scene
            .sim
            .substrate
            .entities
            .get(a)
            .unwrap()
            .air_spatial_bucket(),
        scene
            .sim
            .substrate
            .entities
            .get(b)
            .unwrap()
            .air_spatial_bucket()
    );
    assert_eq!(
        scan(&mut scene),
        [TargetKind::Entity(a), TargetKind::Entity(b)]
    );
    scene.sim.aircraft_tracker_remove(a);
    scene.sim.aircraft_tracker_add(a);
    assert_eq!(
        scan(&mut scene),
        [TargetKind::Entity(b), TargetKind::Entity(a)]
    );
    // Supplied boundary immediately before Update4138C0: current Location
    // changed, retained tracker Cell/bucket did not. Geometry still admits A.
    scene
        .sim
        .substrate
        .entities
        .get_mut(a)
        .unwrap()
        .position
        .set_rx_for_test(26);
    assert_eq!(
        scene
            .sim
            .substrate
            .entities
            .get(a)
            .unwrap()
            .air_tracker_cell(),
        (30, 24)
    );
    assert_eq!(
        scan(&mut scene),
        [TargetKind::Entity(b), TargetKind::Entity(a)]
    );
    scene.sim.aircraft_tracker_update_cell(a, (26, 24));
    let actor = scene.sim.substrate.entities.get(a).unwrap();
    assert_eq!(actor.air_tracker_cell(), (26, 24));
    assert_ne!(
        actor.air_spatial_bucket(),
        scene
            .sim
            .substrate
            .entities
            .get(b)
            .unwrap()
            .air_spatial_bucket()
    );
    let members = scan(&mut scene);
    assert_eq!(members.len(), 2);
    assert!(members.contains(&TargetKind::Entity(a)) && members.contains(&TargetKind::Entity(b)));
}

/// An authored DistributedFire producer exercises the failed ballistic tail
/// using the existing FireAt owner. The null TarCom -> zero launch vector
/// boundary is already executed/consumed by fireat_launch_tests against
/// action_lines_attack_prerequisites.json; a Direct caller supplies the valid
/// explicit argument independently. Stock Cannon physics is unchanged.
#[test]
fn distributed_failed_fireat_remembers_argument_but_unit_attack_bypasses_tail() {
    use crate::sim::combat::world_fire::FireVisit;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &crate::rules::ini_parser::IniFile::from_str("[MTNK]\nDistributedFire=yes\n"),
    ) else {
        return;
    };
    let rules = retail.rules;
    for attack in [false, true] {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1284);
        let tank = sim
            .spawn_object("MTNK", "Americans", 10, 20, 0, &rules)
            .unwrap();
        let target = sim
            .spawn_object("MTNK", "Russians", 13, 20, 0, &rules)
            .unwrap();
        if attack {
            sim.mission_assign_exact(tank, MissionId::from_known(MissionType::Attack), 0)
                .unwrap();
        }
        assert_eq!(
            sim.substrate
                .entities
                .get(tank)
                .unwrap()
                .attack_target
                .target(),
            None
        );
        sim.substrate
            .entities
            .get_mut(tank)
            .unwrap()
            .passive_scan
            .set_passively_acquired_target_for_test(true);
        let before = sim.substrate.entities.get(tank).unwrap().last_fire_frame;
        sim.commit_fire_visit(
            FireVisit::Direct {
                id: tank,
                target: TargetKind::Entity(target),
                weapon_index: 0,
            },
            &rules,
            None,
            FrameEffects::default(),
        );
        assert_eq!(
            sim.projectiles.len(),
            0,
            "the existing ballistic owner rejects the supplied shot"
        );
        let source = sim.substrate.entities.get(tank).unwrap();
        assert_eq!(
            source.last_fire_frame, before,
            "failed launch skips successful firing state"
        );
        assert_eq!(source.attack_target.target(), None);
        assert_eq!(
            source.passive_scan.acquired(),
            attack,
            "only the non-Attack tail calls the same-target NULL setter"
        );
        assert_eq!(
            source.distributed_fire_history(),
            if attack {
                vec![]
            } else {
                vec![Some(TargetKind::Entity(target))]
            }
        );
    }
}

#[test]
fn distributed_building_attack_takes_the_common_history_tail() {
    use crate::sim::combat::world_fire::FireVisit;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &crate::rules::ini_parser::IniFile::from_str("[GAPILL]\nDistributedFire=yes\n"),
    ) else {
        return;
    };
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1284);
    let pillbox = sim
        .spawn_object("GAPILL", "Americans", 10, 20, 0, &rules)
        .unwrap();
    let target = sim
        .spawn_object("E1", "Russians", 12, 20, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    sim.mission_assign_exact(pillbox, MissionId::from_known(MissionType::Attack), 0)
        .unwrap();
    sim.assign_target_represented(
        pillbox,
        Some(TargetKind::Entity(target)),
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
    sim.commit_fire_visit(
        FireVisit::Direct {
            id: pillbox,
            target: TargetKind::Entity(target),
            weapon_index: 0,
        },
        &rules,
        None,
        FrameEffects::default(),
    );
    assert_eq!(sim.projectiles.len(), 1);
    let source = sim.substrate.entities.get(pillbox).unwrap();
    assert_eq!(
        source.attack_target.target(),
        None,
        "native Building exception still clears Attack TarCom"
    );
    assert_eq!(
        source.distributed_fire_history(),
        [Some(TargetKind::Entity(target))]
    );
}

#[test]
fn distributed_collection_flat_entry_clears_but_class_early_return_preserves() {
    use crate::sim::game_entity::DistributedFireCandidate;
    let Some(mut scene) = GuardScene::new(false) else {
        return;
    };
    let target = TargetKind::Entity(scene.targets[0]);
    let entry = DistributedFireCandidate {
        target: Some(target),
        score: 9,
    };
    let source = scene.sim.substrate.entities.get_mut(scene.aegis).unwrap();
    source.replace_distributed_fire_candidates(vec![entry]);
    source.remember_distributed_fire_target(target);
    scene
        .sim
        .greatest_threat_represented(&scene.rules, None, scene.aegis, ScanMission::Hunt, None);
    let source = scene.sim.substrate.entities.get(scene.aegis).unwrap();
    assert!(
        source.distributed_fire_candidates().is_empty(),
        "entered base flat scan clears but has no append sites"
    );
    assert_eq!(source.distributed_fire_history(), [Some(target)]);

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &crate::rules::ini_parser::IniFile::from_str("[COW]\nDistributedFire=yes\n"),
    ) else {
        return;
    };
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1284);
    let cow = sim
        .spawn_object("COW", "Americans", 10, 20, 0, &rules)
        .unwrap();
    let victim = sim
        .spawn_object("MTNK", "Russians", 13, 20, 0, &rules)
        .unwrap();
    let retained = DistributedFireCandidate {
        target: Some(TargetKind::Entity(victim)),
        score: 7,
    };
    let actor = sim.substrate.entities.get_mut(cow).unwrap();
    // Retail ENGINEER is armed by Primary=DefuseKit (and has secondary
    // VirtualScanner), so it correctly reaches the base scan. COW supplies
    // the actual51E288..51E2C4 early return without altering weapon tuning.
    assert_eq!(
        rules.object("ENGINEER").unwrap().primary(),
        Some("DefuseKit")
    );
    let kind = rules.object("COW").unwrap();
    assert_eq!(
        actor.category(),
        crate::map::entities::EntityCategory::Infantry
    );
    assert!(!crate::sim::combat::combat_weapon::is_armed(actor, kind));
    assert!(!kind.infiltrate && !kind.vehicle_thief);
    actor.replace_distributed_fire_candidates(vec![retained]);
    actor.remember_distributed_fire_target(TargetKind::Entity(victim));
    let outcome = crate::sim::combat::combat_targeting::greatest_threat_for_entity(
        &sim.substrate.entities,
        sim.occupancy(),
        &rules,
        &sim.interner,
        cow,
        Some(&sim.fog),
        sim.resolved_terrain.as_ref(),
        true,
        ScanMission::Guard,
        sim.zone_grid.as_ref(),
        LineOfFireInputs::default(),
        Some(&sim),
        None,
    );
    assert!(
        !outcome.foot_wrapper_entered(),
        "native unarmed Infantry class override returns before base"
    );
    assert_eq!(
        sim.greatest_threat_represented(&rules, None, cow, ScanMission::Guard, None),
        None
    );
    let actor = sim.substrate.entities.get(cow).unwrap();
    assert_eq!(actor.distributed_fire_candidates(), [retained]);
    assert_eq!(
        actor.distributed_fire_history(),
        [Some(TargetKind::Entity(victim))]
    );
}
