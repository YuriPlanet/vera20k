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

/// Infantry519B53 and519F44 independently raise Entered. Native519F71
/// reads the entrant's owner after the second callback and Tag transfer.
#[test]
fn engineer_capture_uses_live_owner_after_both_entered_callbacks() {
    use crate::map::entities::EntityCategory;
    use crate::sim::command::Command;
    use crate::sim::house_state::HouseState;
    use crate::sim::world::{SimSoundEvent, TickLane};

    let native =
        crate::test_fixture::json("tools/spatial_oracle/engineer_capture_entered_owner.json");
    assert_eq!(native["source"], "unicorn/gamemd.exe");
    let controls = native["controls"].as_array().unwrap();
    assert_eq!(controls.len(), 4);
    for control in controls {
        assert_eq!(control["runtime_game_mode"], 5);
        let first_change = control["first_change"].as_bool().unwrap();
        let second_change = control["second_change"].as_bool().unwrap();
        let expected_index = usize::try_from(control["owner"].as_u64().unwrap()).unwrap();
        let (mut sim, rules, registry, initial_houses) =
            super::tests::event_ingress_fixture_with_rules(
                "[BuildingTypes]\n1=DEPOT\n[DEPOT]\nStrength=500\nCost=800\n\
                 Capturable=yes\nRepairable=yes\n",
            );
        let country2 = sim.interner.intern("Country2");
        sim.houses
            .get_mut(&initial_houses[2])
            .unwrap()
            .set_country_for_test(Some(country2));
        let house3 = sim.interner.intern("House3");
        let country3 = sim.interner.intern("Country3");
        sim.houses
            .register(HouseState::new(house3, 3, Some(country3), true, 0, 10));
        let houses = [
            initial_houses[0],
            initial_houses[1],
            initial_houses[2],
            house3,
        ];
        for owner in houses {
            sim.houses
                .get_mut(&owner)
                .unwrap()
                .set_is_human_for_test(true);
        }
        sim.session
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        sim.session.game_options_mut_for_test().multi_engineer = false;
        sim.session.game_options_mut_for_test().short_game = false;
        // Keep all four participants alive through the ordinary House visits;
        // the native continuation supplies the same live-House prior.
        for (index, owner) in houses.into_iter().enumerate() {
            let name = sim.interner.resolve(owner).to_owned();
            sim.spawn_object("TAGUNIT", &name, 18 + index as u16, 17, 0, &rules)
                .unwrap();
        }
        let source = if first_change { 7 } else { 0 };
        let first_action = if first_change {
            "36,0,7,0,0,0,0,A,"
        } else {
            ""
        };
        let second_action = if second_change {
            "36,0,3,0,0,0,0,A,"
        } else {
            ""
        };
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nT0=Country0,T1,First,0,1,1,1,0\n\
                 T1=Country{source},<none>,Second,0,1,1,1,0\n\
                 CARRY=Country0,<none>,Carry,1,1,1,1,1\n\
                 [Tags]\nENTRY=2,Entry,T0\nCARRYTAG=2,Carry,CARRY\n\
                 [Events]\nT0=2,1,0,0,28,0,11\nT1=2,1,0,{source},27,0,11\n\
                 CARRY=1,1,0,-1\n[Actions]\n\
                 T0={},{first_action}28,0,11,0,0,0,0,A\n\
                 T1={},{second_action}28,0,13,0,0,0,0,A\nCARRY=0\n",
                1 + usize::from(first_change),
                1 + usize::from(second_change)
            ),
        );
        let building = sim
            .spawn_object("DEPOT", "House2", 16, 16, 0, &rules)
            .unwrap();
        let engineer = sim
            .spawn_object("ENGINEER", "House0", 12, 16, 0, &rules)
            .unwrap();
        let start = crate::sim::movement::ground_pose::position_world_xy(
            &sim.substrate.entities.get(engineer).unwrap().position,
        );
        let depot_type = sim.interner.get("DEPOT").unwrap();
        let engineer_type = sim.interner.get("ENGINEER").unwrap();
        sim.attach_object_trigger_tag(building, Some("ENTRY"));
        sim.attach_object_trigger_tag(engineer, Some("CARRYTAG"));
        let (entry, _) = tag(&mut sim, "ENTRY");
        let (carry, carry_trigger) = tag(&mut sim, "CARRYTAG");
        let entry_triggers = sim.trigger_runtime.tag(entry).unwrap().triggers.clone();
        let live_order: Vec<_> = entry_triggers
            .iter()
            .map(|&index| {
                let trigger = sim.trigger_runtime.trigger(index).unwrap();
                sim.trigger_runtime.definitions.trigger_types[trigger.type_index]
                    .id
                    .as_str()
            })
            .collect();
        assert_eq!(live_order, ["T1", "T0"]);
        assert!(!sim.trigger_runtime.trigger(carry_trigger).unwrap().enabled);
        assert!(sim.apply_command(
            "House0",
            &Command::CaptureBuilding {
                engineer_id: engineer,
                target_building_id: building,
            },
            Some(&rules)
        ));
        let mut moved = false;
        let mut arrivals = Vec::new();
        let mut announcements = Vec::new();
        for frame in 0..256 {
            let trace_start = sim.trigger_runtime.dispatch_trace.len();
            let output = sim
                .advance_app_frame(
                    &[],
                    &rules,
                    Some(&registry),
                    67,
                    TickLane::Ordinary,
                    FrameEffects::default(),
                )
                .unwrap();
            arrivals.extend(
                sim.trigger_runtime.dispatch_trace[trace_start..]
                    .iter()
                    .filter(|&&(index, kind)| index == entry && kind == 1)
                    .map(|_| frame),
            );
            announcements.extend(
                output
                    .sound_events
                    .into_iter()
                    .filter(|event| matches!(event, SimSoundEvent::BuildingCaptured { .. })),
            );
            if let Some(actor) = sim.substrate.entities.get(engineer) {
                moved |=
                    crate::sim::movement::ground_pose::position_world_xy(&actor.position) != start;
            } else {
                break;
            }
        }
        assert!(moved, "the ordinary Walk route must move before arrival");
        assert!(
            sim.substrate.entities.get(engineer).is_none(),
            "Engineer must retire"
        );
        assert!(!sim.logic_order().contains(&engineer));
        assert_eq!(sim.substrate.entities.infantry_registry_len(), 0);
        assert_eq!(arrivals.len(), 2);
        assert_eq!(arrivals[0], arrivals[1]);
        let expected_owner = houses[expected_index];
        let target = sim.substrate.entities.get(building).unwrap();
        assert_eq!(
            target.owner(),
            expected_owner,
            "first={first_change}, second={second_change}"
        );
        assert_eq!(
            target.capture_infantry_type_index.get(),
            rules
                .type_array_index(
                    crate::rules::object_type::ObjectCategory::Infantry,
                    "ENGINEER"
                )
                .unwrap()
        );
        assert_eq!(announcements.len(), 1);
        let SimSoundEvent::BuildingCaptured {
            old_owner,
            new_owner,
            tech_building,
            radar,
            ..
        } = &announcements[0]
        else {
            unreachable!()
        };
        assert_eq!(
            (*old_owner, *new_owner, *tech_building),
            (houses[2], expected_owner, false)
        );
        let radar = radar.as_ref().unwrap();
        assert_eq!((radar.rx, radar.ry), (16, 16));
        let mut actions = Vec::new();
        if first_change {
            actions.push((36, 7));
        }
        actions.push((28, 11));
        if second_change {
            actions.push((36, 3));
        }
        actions.push((28, 13));
        assert_eq!(sim.trigger_runtime.action_trace, actions);
        for (index, owner) in houses.into_iter().enumerate() {
            let house = sim.houses.get(&owner).unwrap();
            assert_eq!(house.building_capture_notified(), index == 2);
            let owns = i32::from(index == expected_index);
            assert_eq!(
                house
                    .tracking()
                    .owned_count(EntityCategory::Structure, depot_type),
                owns
            );
            assert_eq!(
                house
                    .tracking()
                    .active_count(EntityCategory::Structure, depot_type),
                owns
            );
            assert_eq!(house.tracking().buildings(), owns);
            assert_eq!(
                house.base_projection().buildings(),
                if owns == 1 { vec![building] } else { vec![] }
            );
            assert_eq!(
                house
                    .tracking()
                    .owned_count(EntityCategory::Infantry, engineer_type),
                0
            );
            assert_eq!(
                house
                    .tracking()
                    .active_count(EntityCategory::Infantry, engineer_type),
                0
            );
            assert_eq!(house.stats().buildings_lost(), u32::from(index == 2));
            assert_eq!(
                house.stats().buildings_killed(),
                u32::from(index == expected_index)
            );
        }
        assert_eq!(
            sim.trigger_runtime
                .attachments
                .get(&TagAttachment::Object(building)),
            Some(&carry)
        );
        //519F6C copies the attachment. Object5F3B80 removes the consumed
        //Engineer's index without decrementing the retained Tag+2C count.
        assert_eq!(sim.trigger_runtime.tag(carry).unwrap().refs, 2);
        assert!(
            !sim.trigger_runtime
                .attachments
                .contains_key(&TagAttachment::Object(engineer))
        );
        assert_eq!(sim.trigger_runtime.tag(entry).unwrap().refs, 0);
        assert!(!sim.trigger_runtime.trigger(carry_trigger).unwrap().enabled);
        for index in entry_triggers {
            let trigger = sim.trigger_runtime.trigger(index).unwrap();
            let typ = &sim.trigger_runtime.definitions.trigger_types[trigger.type_index];
            let entered_index = typ.events.iter().position(|event| event.kind == 1).unwrap();
            assert_eq!(trigger.completed & (1 << entered_index), 0);
            let source = if typ.id == "T1" && first_change { 1 } else { 0 };
            assert_eq!(trigger.retained_house, Some(houses[source]));
        }
    }
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
            .occupants()
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

/// Infantry519710 raises the building's Entered event with the entering
/// infantryman, before AddGarrisonOccupant522910 conceals him. A rejected
/// admission must not raise it, and the building's House is not the actor.
#[test]
fn garrison_arrival_raises_entered_by_only_for_an_admitted_infantryman() {
    for (actor_house, health, entered) in [(0, 500, true), (1, 500, false), (0, 50, false)] {
        let (mut sim, rules, registry, houses) = super::tests::event_ingress_fixture_with_rules(
            "[Country7]\nMultiplayPassive=yes\n\
             [InfantryTypes]\n0=TAGINF\n[TAGINF]\nStrength=100\nSpeed=4\n\
             Occupier=yes\nSpeedType=Foot\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [BuildingTypes]\n0=TAGGARRISON\n[TAGGARRISON]\nStrength=500\n\
             CanBeOccupied=yes\nMaxNumberOccupants=3\n",
        );
        sim.houses
            .get_mut(&houses[1])
            .unwrap()
            .resolve_multiplay_passive(&rules, Some("Country7"));
        let building = sim
            .spawn_object("TAGGARRISON", "House1", 16, 16, 0, &rules)
            .unwrap();
        let actor_owner = sim.interner.resolve(houses[actor_house]).to_owned();
        let infantry = sim
            .spawn_object("TAGINF", &actor_owner, 15, 16, 0, &rules)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(building)
            .unwrap()
            .health
            .set_for_test(health);
        install(&mut sim, &rules, "1,0,0");
        sim.attach_object_trigger_tag(building, Some("MOVETAG"));
        arrive_at_building(
            &mut sim,
            &rules,
            &registry,
            infantry,
            building,
            crate::sim::mission::MissionType::Capture,
        );
        assert_eq!(
            sim.trigger_runtime.globals_set.contains(&7),
            entered,
            "actor House{actor_house}, building health {health}"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(infantry)
                .unwrap()
                .passenger_role
                .inside_transport_id(),
            (health == 500).then_some(building),
        );
    }
}

/// The physical ALL03 Space Needle links Entered events to full-map reveal
/// on campaign rows 0/1 and six waypoint reveals on row 2. Exercise the
/// selected original trigger chain on flat ground through Capture, Walk and
/// PerCell; this is not a replay of the whole campaign or its authored terrain.
#[test]
fn all03_needle_garrison_runs_the_authored_difficulty_specific_reveal() {
    use crate::map::map_file::MapFile;
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map = MapFile::from_bytes(assets.get_ref("ALL03UMD.MAP").unwrap()).unwrap();
    let named = assets.resolve_ref("ALL03UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL03UMD.INI")
            .unwrap()
            .ini
    });
    let mut reader = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = reader
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&map.ini, None);
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let needle = map
        .entities
        .iter()
        .find(|entity| entity.type_id == "CASEAT01")
        .unwrap();
    assert_eq!(needle.trigger_tag.as_deref(), Some("0BFA637C"));
    assert_eq!(map.tags["0BFA637C"].trigger(), Some("0B7BAC0C"));
    assert_eq!(map.triggers["092589AC"].difficulty(), [true, true, false]);
    assert_eq!(map.triggers["092C7F3C"].difficulty(), [false, false, true]);
    for trigger in ["092589AC", "092C7F3C", "0BFA668C"] {
        assert_eq!(map.events[trigger].conditions()[0].kind, 1);
        assert_eq!(map.events[trigger].conditions()[0].value, 0);
    }
    assert_eq!(map.actions["092589AC"].entries()[0].kind, 16);
    assert_eq!(
        map.actions["092C7F3C"]
            .entries()
            .iter()
            .filter(|entry| entry.kind == 17)
            .count(),
        6
    );
    assert!(rules.object("CASEAT01").unwrap().can_be_occupied);
    let radar = map
        .entities
        .iter()
        .find(|entity| entity.trigger_tag.as_deref() == Some("0BFA6C7C"))
        .unwrap();
    assert_eq!(radar.type_id, "GAAIRC");

    // Keep the authored linked triggers and radar attachment, excluding
    // unrelated mission scripts from this small scene's House polling.
    let triggers = [
        "0BFA3F3C", "0BFA668C", "0B7BAC0C", "092C7F3C", "0B7BCAAC", "092589AC",
    ];
    let tags = ["0BFA6C7C", "0BFA637C"];
    let mut chain = String::new();
    for (section_name, ids) in [
        ("Tags", tags.as_slice()),
        ("Triggers", triggers.as_slice()),
        ("Events", triggers.as_slice()),
        ("Actions", triggers.as_slice()),
    ] {
        chain.push_str(&format!("[{section_name}]\n"));
        let section = map.ini.section(section_name).unwrap();
        for key in section.keys().filter(|key| ids.contains(key)) {
            chain.push_str(&format!("{key}={}\n", section.get_for_test(key).unwrap()));
        }
    }

    for (difficulty, whole_map) in [(0, true), (1, true), (2, false)] {
        let mut sim = Simulation::with_seed(3);
        crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        sim.fog = crate::sim::vision::FogState::new(32, 32);
        // A Size 16x16 diamond fits the arena's 32x32 cell storage.
        sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
            theater: "TEMPERATE".into(),
            fill: "Clear".into(),
            level: 0,
            width: 16,
            height: 16,
            local_left: 0,
            local_top: 0,
            local_width: 16,
            local_height: 16,
        });
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false),
        );
        sim.session
            .initialize_campaign_startup(
                difficulty,
                3,
                None,
                0,
                crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
            )
            .unwrap();
        sim.session.game_options_mut_for_test().short_game = false;
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        crate::sim::scenario_bootstrap::initialize_map_roster_houses(
            &mut sim,
            &roster,
            &rules,
            Some(&map.ini),
        );
        let player = sim.interner.get("Americans").unwrap();
        sim.session.set_current_house_for_test(Some(player));
        sim.houses
            .get_mut(&player)
            .unwrap()
            .set_is_human_for_test(true);
        sim.session.install_waypoints(&map.waypoints);
        install_event_ingress_map(&mut sim, &rules, &chain);
        let building = sim
            .spawn_object(&needle.type_id, &needle.owner, 16, 16, 0, &rules)
            .unwrap();
        sim.attach_object_trigger_tag(building, needle.trigger_tag.as_deref());
        let radar_building = sim
            .spawn_object(&radar.type_id, &radar.owner, 7, 17, 0, &rules)
            .unwrap();
        sim.attach_object_trigger_tag(radar_building, radar.trigger_tag.as_deref());
        sim.spawn_object("GAPOWR", "Americans", 11, 20, 0, &rules)
            .unwrap();
        let infantry = sim
            .spawn_object("E1", "Americans", 13, 16, 0, &rules)
            .unwrap();
        assert!(!sim.fog.whole_map_revealed_for_owner(player));
        assert!(!sim.fog.is_cell_revealed(player, 24, 16));
        assert!(sim.apply_command(
            "Americans",
            &crate::sim::command::Command::CaptureBuilding {
                engineer_id: infantry,
                target_building_id: building,
            },
            Some(&rules)
        ));
        let mut garrisoned = false;
        for _ in 0..256 {
            sim.advance_app_frame(
                &[],
                &rules,
                Some(&registry),
                67,
                crate::sim::world::TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
            if sim
                .substrate
                .entities
                .get(infantry)
                .unwrap()
                .passenger_role
                .inside_transport_id()
                == Some(building)
            {
                garrisoned = true;
                break;
            }
        }
        assert!(
            garrisoned,
            "difficulty {difficulty}: ordinary Capture must enter the Needle"
        );
        assert_eq!(
            sim.fog.whole_map_revealed_for_owner(player),
            whole_map,
            "difficulty {difficulty}"
        );
        assert_eq!(sim.fog.is_cell_revealed(player, 24, 16), whole_map);
        let actions: Vec<_> = sim
            .trigger_runtime
            .action_trace
            .iter()
            .map(|row| row.0)
            .collect();
        assert_eq!(
            actions.iter().filter(|&&kind| kind == 16).count(),
            usize::from(whole_map)
        );
        assert_eq!(
            actions.iter().filter(|&&kind| kind == 17).count(),
            if whole_map { 0 } else { 6 }
        );
        assert!(
            actions.contains(&53),
            "entry also enables the authored radar reward"
        );
        for _ in 0..16 {
            sim.advance_app_frame(
                &[],
                &rules,
                Some(&registry),
                67,
                crate::sim::world::TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        }
        assert_eq!(sim.fog.whole_map_revealed_for_owner(player), whole_map);
        assert_eq!(sim.fog.is_cell_revealed(player, 24, 16), whole_map);
    }
}
