//! Original Steam gamemd:12 HasOccurred23 and14 HasOccurred34 controls.
//! Native34 executes the real distance kernel and actor GetCoords, with an
//! explicit waypoint-coordinate sink. Separate68BD00 controls cover its map.
use super::tests::{event_ingress_fixture, install_event_ingress_map, spawn_event_ingress_unit};
use super::*;
use crate::map::waypoints::Waypoint;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::sim::team_script_vm::{
    TeamScriptAction, TeamScriptDefinition, TeamScriptMember, TeamTaskForceDefinition,
    TeamTaskForceEntry, TeamTypeDefinition,
};
use std::collections::HashMap;

fn rng(sim: &Simulation) -> [String; 3] {
    [
        sim.main_rng.native_state_hex(),
        sim.scenario_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

fn tag(sim: &mut Simulation, name: &str) -> (usize, usize) {
    let typ = sim.trigger_runtime.definitions.tag_type(name).unwrap();
    let tag = sim.shared_trigger_tag(typ);
    (tag, sim.trigger_runtime.tag(tag).unwrap().triggers[0])
}

fn install(sim: &mut Simulation, rules: &RuleSet, event: &str) -> (usize, usize) {
    install_event_ingress_map(
        sim,
        rules,
        &format!(
            "[Triggers]\nMOVE=Country0,<none>,Move,0,1,1,1,0\n[Tags]\nMOVETAG=0,Move,MOVE\n[Events]\nMOVE=1,{event}\n[Actions]\nMOVE=1,28,0,7,0,0,0,0,A\n"
        ),
    );
    tag(sim, "MOVETAG")
}

fn team(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    name: Option<&str>,
    head: bool,
    leaving: bool,
) -> u64 {
    let cell = (15 + sim.team_script_vm.teams_in_order().count() as u16, 15);
    let actor = spawn_event_ingress_unit(sim, rules, owner, cell);
    let member_type = crate::sim::team_script_vm::member_type_identity(
        sim.substrate.entities.get(actor).unwrap(),
    );
    let script = sim.interner.intern("CONTROL_SCRIPT");
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: script,
        source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
        actions: vec![TeamScriptAction {
            action_id: 5,
            argument: 999,
        }],
    });
    let id = if let Some(name) = name {
        let typ = sim.interner.intern(name);
        let task = sim.interner.intern("CONTROL_TASK_FORCE");
        sim.team_script_vm
            .register_task_force(TeamTaskForceDefinition {
                id: task,
                source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
                group: -1,
                entries: vec![TeamTaskForceEntry {
                    member_type,
                    count: 1,
                }],
            });
        sim.team_script_vm.register_team_type(TeamTypeDefinition {
            id: typ,
            script_id: script,
            task_force_id: task,
            priority: 0,
            is_base_defense: false,
            suicide: false,
            aggressive: false,
            combined_movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
            base_zone_relation_enforced: true,
            transport_crossing_required: false,
        });
        sim.team_script_vm.create_team_from_type(
            owner,
            typ,
            &[TeamScriptMember {
                entity_id: actor,
                member_type,
            }],
            0,
        )
    } else {
        sim.team_script_vm
            .create_team(owner, script, vec![actor], 0)
    };
    if leaving {
        sim.team_script_vm.mark_member_team_leaving_map(actor);
    }
    if !head {
        sim.team_remove_member(id, actor, true, rules, FrameEffects::default());
    }
    id
}

#[test]
fn twelve_original_team_predicates_keep_type_head_leaving_house_and_persistence_semantics() {
    // Tuple: prior Teams(type,head,leaving), retained Event type, House,
    // raised kind, bypass, initial persistence, original result/persistence.
    let cases: &[(
        &[(Option<&str>, bool, bool)],
        Option<&str>,
        bool,
        i32,
        bool,
        bool,
        bool,
        bool,
    )] = &[
        (&[], Some("MATCH"), true, 23, false, false, false, false),
        (
            &[(Some("MATCH"), false, true)],
            Some("MATCH"),
            true,
            23,
            false,
            false,
            true,
            true,
        ),
        (
            &[(Some("MATCH"), true, true)],
            Some("MATCH"),
            true,
            23,
            false,
            false,
            false,
            false,
        ),
        (
            &[(Some("MATCH"), false, false)],
            Some("MATCH"),
            true,
            23,
            false,
            false,
            false,
            false,
        ),
        (
            &[(Some("OTHER"), false, true)],
            Some("MATCH"),
            true,
            23,
            false,
            false,
            false,
            false,
        ),
        (
            &[(Some("OTHER"), false, true), (Some("MATCH"), false, true)],
            Some("MATCH"),
            true,
            23,
            false,
            false,
            true,
            true,
        ),
        (
            &[(None, false, true)],
            None,
            true,
            23,
            false,
            false,
            true,
            true,
        ),
        (&[], Some("MATCH"), false, 23, false, false, true, false),
        (
            &[(Some("MATCH"), false, true)],
            Some("MATCH"),
            false,
            23,
            false,
            false,
            true,
            false,
        ),
        (
            &[(Some("MATCH"), false, true)],
            Some("MATCH"),
            true,
            8,
            false,
            false,
            false,
            false,
        ),
        (
            &[(Some("MATCH"), false, true)],
            Some("MATCH"),
            true,
            23,
            true,
            false,
            false,
            false,
        ),
        (&[], Some("MATCH"), true, 23, false, true, false, true),
    ];
    for &(teams, wanted, house, raised, bypass, mut persistence, result, after) in cases {
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        let raw = wanted.map_or_else(|| "23,0,999".to_owned(), |name| format!("23,1,{name}"));
        let (_, live) = install(&mut sim, &rules, &raw);
        for &(name, head, leaving) in teams {
            team(&mut sim, &rules, houses[0], name, head, leaving);
        }
        let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[typ].events[0].retained_house =
            Some(houses[2]);
        let before = rng(&sim);
        let timer = sim.trigger_runtime.trigger(live).unwrap().timer;
        let mut call = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: raised,
                    ..Default::default()
                },
                house.then_some(houses[0]),
                &mut persistence,
                &rules,
                live,
            )
        };
        let actual = if bypass {
            sim.with_buildup_bypass(call)
        } else {
            call(&mut sim)
        };
        assert_eq!(actual, result, "{teams:?} house{house} kind{raised}");
        assert_eq!(persistence, after);
        assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[typ].events[0].retained_house,
            Some(houses[2])
        );
        assert_eq!(rng(&sim), before);
    }
}

#[test]
fn fourteen_original_near_waypoint_distance_controls_reuse_the_numeric_owner() {
    for (delta, expected) in [
        ([0, 0, 0], true),
        ([1279, 0, 0], true),
        ([1280, 0, 0], true),
        ([1281, 0, 0], true),
        ([1282, 0, 0], false),
        ([1283, 0, 0], false),
        ([768, 1024, 0], true),
        ([769, 1024, 0], true),
        ([0, 0, 1281], true),
        ([-769, -1024, 0], true),
        ([i32::MAX, 0, 0], false),
        ([i32::MIN, 0, 0], true),
    ] {
        assert_eq!(
            crate::util::native_x87::distance_3d_leptons(delta, [0, 0, 0]) <= 1280,
            expected
        );
        // Position's legacy packed/subcell adapter cannot represent the i32
        // extremes. Those two original numeric controls stay at the kernel
        // boundary; the other ten reach production HasOccurred.
        if delta[0] == i32::MIN || delta[0] == i32::MAX {
            continue;
        }
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        let (_, live) = install(&mut sim, &rules, "34,0,7");
        sim.session.install_waypoints(&HashMap::from([(
            7,
            Waypoint {
                index: 7,
                rx: 0,
                ry: 0,
            },
        )]));
        let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
        sim.substrate
            .entities
            .get_mut(actor)
            .unwrap()
            .position
            .put_location(DriveCoord {
                x: 128 + delta[0],
                y: 128 + delta[1],
                z: delta[2],
            });
        let before = rng(&sim);
        let mut persistence = false;
        let input = TagEventInput {
            kind: 34,
            actor: Some(actor),
            ..Default::default()
        };
        assert_eq!(
            sim.has_occurred(0, input, Some(houses[0]), &mut persistence, &rules, live),
            expected,
            "{delta:?}"
        );
        assert!(!persistence);
        if delta == [0, 0, 0] {
            assert!(!sim.has_occurred(
                0,
                TagEventInput { kind: 8, ..input },
                Some(houses[0]),
                &mut persistence,
                &rules,
                live
            ));
            assert!(!sim.with_buildup_bypass(|sim| sim.has_occurred(
                0,
                input,
                Some(houses[0]),
                &mut persistence,
                &rules,
                live
            )));
        }
        assert_eq!(rng(&sim), before);
    }
}

#[test]
fn empty_leaving_team_recalc_raises_before_destruction_and_cleans_one_shot_tags() {
    let (mut sim, rules, registry, houses) = event_ingress_fixture();
    let (tag, _) = install(&mut sim, &rules, "23,1,MATCH");
    let id = team(&mut sim, &rules, houses[0], Some("MATCH"), false, true);
    assert!(sim.team_script_vm.team(id).is_some());
    let before = rng(&sim);
    sim.run_team_ai_pass(&rules, Some(&registry), FrameEffects::default());
    assert!(sim.team_script_vm.team(id).is_none());
    sim.flush_pending_delete();
    assert!(sim.trigger_runtime.tag(tag).is_none());
    assert!(sim.trigger_runtime.globals_set.contains(&7));
    assert_eq!(rng(&sim), before);
}

#[test]
fn ordinary_per_cell_arrival_raises_attached_near_event_before_next_poll_and_survives_save() {
    let (mut sim, rules, registry, houses) = event_ingress_fixture();
    let (tag, _) = install(&mut sim, &rules, "34,0,7");
    sim.session.install_waypoints(&HashMap::from([(
        7,
        Waypoint {
            index: 7,
            rx: 15,
            ry: 15,
        },
    )]));
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    sim.attach_object_trigger_tag(actor, Some("MOVETAG"));
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Near event", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    loaded.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
    loaded.reconstruct_cellclass_dummy_for_map_resize();
    loaded.restore_after_snapshot_load().unwrap();
    loaded.rebuild_caches_after_load(
        sim.resolved_terrain.as_ref().unwrap().clone(),
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    loaded.resolve_type_handles(&rules);
    for sim in [&mut sim, &mut loaded] {
        let before = rng(sim);
        sim.per_cell_process(
            actor,
            crate::sim::movement::PerCellReason::TurnComplete,
            &rules,
            Some(&registry),
            FrameEffects::default(),
        )
        .unwrap();
        assert!(!sim.trigger_runtime.globals_set.contains(&7));
        sim.per_cell_process(
            actor,
            crate::sim::movement::PerCellReason::Arrival,
            &rules,
            Some(&registry),
            FrameEffects::default(),
        )
        .unwrap();
        assert!(sim.trigger_runtime.globals_set.contains(&7));
        sim.flush_pending_delete();
        assert!(sim.trigger_runtime.tag(tag).is_none());
        assert_eq!(rng(sim), before);
    }
}

/// Infantry PerCellProcess's Enter arm raises event 1 (Entered by) on the
/// Tag of what he enters, with him as the actor (`0x0051A258..0x0051A26A`),
/// before the building arm's DOCK_NOW takes him in.
#[test]
fn an_infantryman_entering_a_bio_reactor_raises_its_entered_by_event() {
    let (mut sim, rules, registry, houses) = super::tests::event_ingress_fixture_with_rules(
        "[InfantryTypes]\n0=TAGINF\n[TAGINF]\nStrength=100\nSpeed=4\nSpeedType=Foot\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [BuildingTypes]\n0=TAGBIO\n[TAGBIO]\nStrength=500\nInfantryAbsorb=yes\n\
         Passengers=5\nSizeLimit=15\n",
    );
    let owner = sim.interner.resolve(houses[0]).to_owned();
    let reactor = sim
        .spawn_object("TAGBIO", &owner, 16, 16, 0, &rules)
        .unwrap();
    let infantry = sim
        .spawn_object("TAGINF", &owner, 17, 16, 0, &rules)
        .unwrap();
    install(&mut sim, &rules, "1,0,-1");
    sim.attach_object_trigger_tag(reactor, Some("MOVETAG"));
    sim.substrate
        .mission_assign_exact(
            infantry,
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Enter),
            0,
        )
        .unwrap();
    // The Infantry setter's Enter arm HELLOs the reactor (`0x0051ACD4`).
    let _ = sim.assign_destination_represented(
        infantry,
        Some(crate::sim::components::NavTargetRef::object(reactor)),
        &rules,
        Some(&registry),
        FrameEffects::default(),
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(infantry)
            .unwrap()
            .radio_contacts
            .slot(0),
        Some(reactor)
    );
    // A supplied arrival on the reactor's cell through the shared Foot writer.
    sim.foot_set_location_marked(
        infantry,
        DriveCoord {
            x: 16 * 256 + 128,
            y: 16 * 256 + 128,
            z: 0,
        },
        &rules,
        Some(&registry),
        FrameEffects::default(),
    );
    sim.per_cell_process(
        infantry,
        crate::sim::movement::PerCellReason::Arrival,
        &rules,
        Some(&registry),
        FrameEffects::default(),
    )
    .unwrap();
    assert!(sim.trigger_runtime.globals_set.contains(&7));
    assert_eq!(
        sim.substrate
            .entities
            .get(infantry)
            .unwrap()
            .passenger_role
            .inside_transport_id(),
        Some(reactor)
    );
}

/// Puts `infantry` on `mission`, heading for `building`, standing on the
/// building's cell through the shared Foot writer, then runs its arrival.
fn arrive_at_building(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    infantry: u64,
    building: u64,
    mission: crate::sim::mission::MissionType,
) {
    let (rx, ry) = {
        let building = sim.substrate.entities.get(building).unwrap();
        (building.position.rx(), building.position.ry())
    };
    sim.substrate
        .mission_assign_exact(
            infantry,
            crate::sim::mission::MissionId::from_known(mission),
            0,
        )
        .unwrap();
    sim.substrate
        .entities
        .get_mut(infantry)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::object(building)));
    sim.foot_set_location_marked(
        infantry,
        DriveCoord {
            x: i32::from(rx) * 256 + 128,
            y: i32::from(ry) * 256 + 128,
            z: 0,
        },
        rules,
        Some(registry),
        FrameEffects::default(),
    );
    sim.per_cell_process(
        infantry,
        crate::sim::movement::PerCellReason::Arrival,
        rules,
        Some(registry),
        FrameEffects::default(),
    )
    .unwrap();
}

/// Infantry PerCellProcess's occupier arm raises event 1 (Entered by) on the
/// Tag of the building it garrisons, with the infantryman as the actor
/// (`0x00519710..0x00519725`), before AddGarrisonOccupant (`0x0051972D`).
/// XArena's CATECH01 crates hang on this event.
#[test]
fn an_occupier_garrisoning_a_building_raises_its_entered_by_event() {
    let (mut sim, rules, registry, houses) = super::tests::event_ingress_fixture_with_rules(
        "[InfantryTypes]\n0=TAGOCC\n[TAGOCC]\nStrength=100\nSpeed=4\nSpeedType=Foot\n\
         Occupier=yes\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [BuildingTypes]\n0=TAGCIV\n[TAGCIV]\nStrength=500\nCanBeOccupied=yes\n\
         MaxNumberOccupants=5\n",
    );
    let owner = sim.interner.resolve(houses[0]).to_owned();
    let building = sim
        .spawn_object("TAGCIV", &owner, 16, 16, 0, &rules)
        .unwrap();
    let infantry = sim
        .spawn_object("TAGOCC", &owner, 17, 16, 0, &rules)
        .unwrap();
    install(&mut sim, &rules, "1,0,-1");
    sim.attach_object_trigger_tag(building, Some("MOVETAG"));

    arrive_at_building(
        &mut sim,
        &rules,
        &registry,
        infantry,
        building,
        crate::sim::mission::MissionType::Capture,
    );

    assert!(sim.trigger_runtime.globals_set.contains(&7));
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .count(),
        1
    );
}

/// Infantry PerCellProcess's C4 arm raises event 1 (Entered by) on the Tag
/// of its NavCom building on every arrival, with the planter as the actor
/// (`0x0051A507..0x0051A51C`), before the Selling and invulnerability
/// refusals (`0x0051A525..`). XBermuda's CAAIRP ends its storms this way.
#[test]
fn a_c4_planter_arriving_at_a_building_raises_its_entered_by_event() {
    for selling in [false, true] {
        let (mut sim, rules, registry, houses) = super::tests::event_ingress_fixture_with_rules(
            "[InfantryTypes]\n0=TAGC4\n[TAGC4]\nStrength=100\nSpeed=4\nSpeedType=Foot\n\
             C4=yes\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [BuildingTypes]\n0=TAGAIR\n[TAGAIR]\nStrength=500\n",
        );
        let (ours, theirs) = (
            sim.interner.resolve(houses[0]).to_owned(),
            sim.interner.resolve(houses[1]).to_owned(),
        );
        let building = sim
            .spawn_object("TAGAIR", &theirs, 16, 16, 0, &rules)
            .unwrap();
        let infantry = sim.spawn_object("TAGC4", &ours, 17, 16, 0, &rules).unwrap();
        install(&mut sim, &rules, "1,0,-1");
        sim.attach_object_trigger_tag(building, Some("MOVETAG"));
        if selling {
            sim.substrate
                .mission_assign_exact(
                    building,
                    crate::sim::mission::MissionId::from_known(
                        crate::sim::mission::MissionType::Selling,
                    ),
                    0,
                )
                .unwrap();
        }

        arrive_at_building(
            &mut sim,
            &rules,
            &registry,
            infantry,
            building,
            crate::sim::mission::MissionType::Sabotage,
        );

        assert!(
            sim.trigger_runtime.globals_set.contains(&7),
            "selling {selling}"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(building)
                .unwrap()
                .pending_c4_detonation
                .pending()
                .is_some(),
            !selling
        );
    }
}
