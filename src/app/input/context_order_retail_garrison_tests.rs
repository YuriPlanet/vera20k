//! Physical retail rules/ART/map load and the ordinary garrison input route.
//!
//! The Neutral map garrisons come from the loaded map; the GIs are spawned.
//! The click producer and hover predicate, ordinary local event codec and
//! scheduler run through production owners, then the bound runtime walks
//! Capture GIs in. Mouse picking and physical audio output are outside this test.

use super::*;
use crate::app::input::cursor::{
    ActionDistanceTarget, capability_cursor_for_hover, select_best_for_action,
};
use crate::app::types::CursorFeedbackKind;
use crate::map::entities::EntityCategory;
use crate::sim::passenger::PassengerRole;
use crate::sim::world::TickLane;
use serde_json::Value;

fn retail_garrison_scene() -> Option<(crate::headless_scenario::HeadlessScenario, u64)> {
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()?;
    let profile: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/map_observation.building-opening.example.json",
    ))
    .unwrap();
    let launch = serde_json::from_value(profile["launch"].clone()).unwrap();
    let descriptor =
        crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(launch).unwrap();
    let scene =
        crate::headless_scenario::load_with_launch(&retail, "XMP03T4.MAP", 0x1234_5678, descriptor)
            .unwrap();
    let sim = &scene.runtime.simulation;
    let rules = &scene.runtime.resources.rules;
    assert_eq!(rules.object("CASTL03").unwrap().foundation, "4x4");
    assert_eq!(rules.object("CASTL03").unwrap().max_number_occupants, 10);
    assert!(rules.object("E1").unwrap().occupier);
    let castle = sim
        .entities()
        .values()
        .find(|e| {
            e.category() == EntityCategory::Structure && sim.resolve(e.type_ref()) == "CASTL03"
        })
        .expect("XMP03T4 places one CASTL03");
    assert_eq!((castle.position.rx, castle.position.ry), (82, 112));
    assert_eq!(sim.resolve(castle.owner()), "Neutral");
    let castle = castle.stable_id();
    Some((scene, castle))
}

fn advance_garrison_frame(runtime: &mut crate::sim::runtime::SimRuntime) {
    let due = runtime.simulation.take_due_commands();
    runtime
        .advance_frame(
            &due,
            crate::app::types::SIM_TICK_MS,
            TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
}

fn queue_ordinary_order(runtime: &mut crate::sim::runtime::SimRuntime, order: Command) {
    let sim = &runtime.simulation;
    let envelope =
        CommandEnvelope::new(sim.session.current_house.unwrap(), sim.session.tick, order);
    let envelope =
        crate::app::input::commands::roundtrip_ordinary_local_megamission(sim, envelope).unwrap();
    runtime.simulation.queue_commands(vec![envelope]);
}

/// Five GIs south-east of CASTL03 (`Foundation=4x4`) on XMP03T4. Their walk
/// targets the building's `+0x4C` coordinate, the foundation centre, so the
/// first foundation cell they step on is two cells from the origin.
/// `InfantryClass::PerCellProcess` `0x00519675..0x00519739` occupies there; the
/// earlier Chebyshev-1-from-origin admission left every one of them standing
/// inside the foundation in mission Enter.
#[test]
fn retail_gi_group_boards_a_4x4_garrison_from_its_far_side() {
    let Some((mut scene, castle)) = retail_garrison_scene() else {
        return;
    };
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let rules = &runtime.resources.rules;
    let position = &runtime.simulation.entities().get(castle).unwrap().position;
    let (rx, ry) = (position.rx, position.ry);

    let mut gis = Vec::new();
    for (dx, dy) in [(6, 6), (5, 5), (6, 5), (7, 5), (5, 6)] {
        gis.push(
            runtime
                .simulation
                .spawn_object("E1", &owner_name, rx + dx, ry + dy, 0, rules)
                .expect("open ground south-east of the castle"),
        );
    }

    // The click route: every GI passes CanBeOccupiedBy and the cursor is Enter.
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::EnemyStructure,
        stable_id: castle,
    };
    for &gi in &gis {
        assert!(crate::sim::passenger::can_enter_transport(
            &runtime.simulation,
            rules,
            gi,
            castle
        ));
    }
    let best = select_best_for_action(
        &runtime.simulation,
        &gis,
        ActionDistanceTarget::Object(castle),
        Some(rules),
    );
    assert_eq!(
        capability_cursor_for_hover(&runtime.simulation, &gis, best, &hover, Some(rules), None),
        CursorFeedbackKind::Enter
    );
    let orders = transport_entry_orders(&runtime.simulation, rules, &gis, castle);
    assert_eq!(orders.len(), gis.len());
    for (order, &gi) in orders.iter().zip(&gis) {
        assert_eq!(
            order,
            &Command::CaptureBuilding {
                engineer_id: gi,
                target_building_id: castle,
            },
            "native Infantry What_Action51E6A8 answers Capture action9"
        );
        assert_eq!(order_voice_key(order), Some("VoiceCapture"));
    }
    let envelopes: Vec<_> = orders
        .into_iter()
        .map(|order| {
            let envelope = CommandEnvelope::new(owner, runtime.simulation.session.tick, order);
            crate::app::input::commands::roundtrip_ordinary_local_megamission(
                &runtime.simulation,
                envelope,
            )
            .unwrap()
        })
        .collect();
    runtime.simulation.queue_commands(envelopes);
    let castle_strength = rules.object("CASTL03").unwrap().strength;

    let mut boarded_frame = None;
    for frame in 0..600 {
        let due = runtime.simulation.take_due_commands();
        runtime
            .advance_frame(
                &due,
                crate::app::types::SIM_TICK_MS,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        let sim = &runtime.simulation;
        for &gi in &gis {
            let e = sim.entities().get(gi).unwrap();
            assert!(
                e.attack_target.is_none(),
                "frame {frame}: GI {gi} must not target the castle while entering"
            );
        }
        let cargo = sim
            .entities()
            .get(castle)
            .and_then(|b| b.passenger_role.cargo())
            .map(|c| c.count());
        if cargo == Some(gis.len() as u32) {
            boarded_frame = Some(frame);
            break;
        }
    }
    let sim = &runtime.simulation;
    assert!(
        boarded_frame.is_some(),
        "all five GIs board within 600 frames; roles: {:?}",
        gis.iter()
            .map(|&gi| {
                let e = sim.entities().get(gi).unwrap();
                (
                    gi,
                    (e.position.rx, e.position.ry),
                    e.mission.effective().known(),
                )
            })
            .collect::<Vec<_>>()
    );
    for &gi in &gis {
        let occupant = sim.entities().get(gi).unwrap();
        assert!(matches!(
            occupant.passenger_role,
            PassengerRole::Inside { transport_id, .. } if transport_id == castle
        ));
        assert!(occupant.lifecycle.in_limbo);
        assert!(!occupant.lifecycle.cell_marked);
        assert!(!occupant.in_logic_vector);
        assert!(!sim.live_object_order_snapshot().contains(&gi));
    }
    let castle_entity = sim.entities().get(castle).unwrap();
    assert_eq!(castle_entity.health.current(), castle_strength);
    assert_eq!(sim.resolve(castle_entity.owner()), owner_name);
}

/// Supply target state changes after an ordinary click has started walking.
/// Damage and another house's entry are priors here, not tests of combat or
/// capture by that house. The production Walk reaches PerCell's repeated
/// CanBeOccupiedBy457CE0 / refusal5196E5..519700; Stop replaces the order.
#[test]
fn retail_capture_walk_rechecks_admission_and_respects_a_replacement_order() {
    for control in ["red_health", "full", "other_owner", "stop"] {
        let Some((mut scene, castle)) = retail_garrison_scene() else {
            return;
        };
        let runtime = &mut scene.runtime;
        let owner = runtime.simulation.session.current_house.unwrap();
        let owner_name = runtime.simulation.resolve(owner).to_owned();
        let gi = runtime
            .simulation
            .spawn_object("E1", &owner_name, 88, 118, 0, &runtime.resources.rules)
            .unwrap();
        let orders =
            transport_entry_orders(&runtime.simulation, &runtime.resources.rules, &[gi], castle);
        assert_eq!(orders.len(), 1, "{control}: initially admitted");
        queue_ordinary_order(runtime, orders.into_iter().next().unwrap());
        for _ in 0..4 {
            advance_garrison_frame(runtime);
        }
        let actor = runtime.simulation.entities().get(gi).unwrap();
        assert_eq!(
            actor.mission.effective().known(),
            Some(crate::sim::mission::MissionType::Capture)
        );
        assert!(matches!(actor.passenger_role, PassengerRole::None));
        assert_eq!(
            actor.navigation.nav_com,
            Some(crate::sim::components::NavTargetRef::Building { id: castle })
        );

        let expected_cargo = match control {
            "red_health" => {
                runtime
                    .simulation
                    .entities_mut()
                    .get_mut(castle)
                    .unwrap()
                    .health
                    .set_for_test(1);
                0
            }
            "full" | "other_owner" => {
                let (count, occupant_owner) = if control == "full" {
                    (
                        runtime
                            .resources
                            .rules
                            .object("CASTL03")
                            .unwrap()
                            .max_number_occupants,
                        owner_name.as_str(),
                    )
                } else {
                    // Computer1 is the allied France slot in this launch. Its
                    // occupant retains the other owner without firing at us.
                    (1, "Computer1")
                };
                for _ in 0..count {
                    let occupant = runtime
                        .simulation
                        .spawn_object("E1", occupant_owner, 89, 118, 0, &runtime.resources.rules)
                        .unwrap();
                    assert!(crate::sim::passenger::add_garrison_occupant(
                        &mut runtime.simulation,
                        &runtime.resources.rules,
                        occupant,
                        castle,
                        None,
                        crate::sim::world::FrameEffects::default(),
                    ));
                }
                count
            }
            "stop" => {
                queue_ordinary_order(runtime, Command::Stop { entity_id: gi });
                0
            }
            _ => unreachable!(),
        };
        let mut left_order = false;
        for _ in 0..600 {
            advance_garrison_frame(runtime);
            let actor = runtime.simulation.entities().get(gi).unwrap();
            assert!(
                !actor.lifecycle.in_limbo,
                "{control}: refused GI stays visible"
            );
            assert!(
                matches!(actor.passenger_role, PassengerRole::None),
                "{control}"
            );
            let outside =
                !(82..86).contains(&actor.position.rx) || !(112..116).contains(&actor.position.ry);
            if actor.mission.effective().known() != Some(crate::sim::mission::MissionType::Capture)
                && outside
            {
                left_order = true;
                break;
            }
        }
        let actor = runtime.simulation.entities().get(gi).unwrap();
        assert!(
            left_order,
            "{control}: GI must leave Capture and the foundation; at {:?}, NavCom {:?}",
            actor.position, actor.navigation.nav_com
        );
        assert!(
            actor.lifecycle.cell_marked && actor.in_logic_vector,
            "{control}"
        );
        assert!(
            runtime
                .simulation
                .live_object_order_snapshot()
                .contains(&gi),
            "{control}"
        );
        let building = runtime.simulation.entities().get(castle).unwrap();
        assert_eq!(
            building.passenger_role.passenger_count(),
            expected_cargo,
            "{control}"
        );
        if control == "other_owner" {
            assert_eq!(runtime.simulation.resolve(building.owner()), "Computer1");
        }
    }
}

/// The same retail click producer keeps a carrier on Enter7; migrating
/// garrison action9 must not change the IFV's admission/Radio route.
#[test]
fn retail_gi_ifv_click_keeps_enter() {
    let Some((mut scene, _)) = retail_garrison_scene() else {
        return;
    };
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let rules = &runtime.resources.rules;
    let gi = runtime
        .simulation
        .spawn_object("E1", &owner_name, 88, 118, 0, rules)
        .unwrap();
    // Vehicle placement has different terrain admission from the GI. This
    // control tests its click route, with an already admitted nearby carrier.
    let ifv = [(89, 117), (88, 117), (87, 117), (86, 117), (89, 118)]
        .into_iter()
        .find_map(|(rx, ry)| {
            runtime
                .simulation
                .spawn_object("FV", &owner_name, rx, ry, 0, rules)
        })
        .expect("a vehicle-admitted cell near the castle");
    assert_eq!(
        transport_entry_orders(&runtime.simulation, rules, &[gi], ifv),
        vec![Command::EnterTransport {
            passenger_id: gi,
            transport_id: ifv
        }]
    );
}
