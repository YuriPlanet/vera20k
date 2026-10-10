//! Physical retail rules/ART/map load and the ordinary garrison input route.
//!
//! The Neutral map garrisons come from the loaded map; the GIs are spawned.
//! The click decision (predicate, cursor) and the EnterTransport order run
//! through the production owners, then the bound runtime walks the GIs in.
//! Mouse picking, the native event-byte codec and audio output are outside
//! this test.

use super::*;
use crate::app::input::cursor::{
    ActionDistanceTarget, capability_cursor_for_hover, select_best_for_action,
};
use crate::app::types::CursorFeedbackKind;
use crate::map::entities::EntityCategory;
use crate::sim::passenger::PassengerRole;
use crate::sim::world::TickLane;
use serde_json::Value;

/// Five GIs south-east of CASTL03 (`Foundation=4x4`) on XMP03T4. Their walk
/// targets the building's `+0x4C` coordinate, the foundation centre, so the
/// first foundation cell they step on is two cells from the origin.
/// `InfantryClass::PerCellProcess` `0x0051A27F..0x0051A292` boards there; the
/// earlier Chebyshev-1-from-origin admission left every one of them standing
/// inside the foundation in mission Enter.
#[test]
#[ignore = "physical XMP03T4, retail rules/ART binding and the Enter walk"]
fn retail_gi_group_boards_a_4x4_garrison_from_its_far_side() {
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").unwrap());
    let profile: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/map_observation.building-opening.example.json",
    ))
    .unwrap();
    let launch = serde_json::from_value(profile["launch"].clone()).unwrap();
    let descriptor =
        crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(launch).unwrap();
    let mut scene =
        crate::headless_scenario::load_with_launch(&retail, "XMP03T4.MAP", 0x1234_5678, descriptor)
            .unwrap();
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let rules = &runtime.resources.rules;
    assert_eq!(rules.object("CASTL03").unwrap().foundation, "4x4");
    assert_eq!(rules.object("CASTL03").unwrap().max_number_occupants, 10);
    assert!(rules.object("E1").unwrap().occupier);

    let (castle, rx, ry) = runtime
        .simulation
        .entities()
        .values()
        .find(|e| {
            e.category == EntityCategory::Structure
                && runtime.simulation.resolve(e.type_ref()) == "CASTL03"
        })
        .map(|e| (e.stable_id(), e.position.rx, e.position.ry))
        .expect("XMP03T4 places one CASTL03");
    assert_eq!((rx, ry), (82, 112));
    assert_eq!(
        runtime
            .simulation
            .resolve(runtime.simulation.entities().get(castle).unwrap().owner()),
        "Neutral"
    );

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
        assert!(crate::sim::passenger::can_entity_enter_garrison(
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
    let envelopes: Vec<_> = gis
        .iter()
        .map(|&gi| {
            let envelope = CommandEnvelope::new(
                owner,
                runtime.simulation.session.tick,
                Command::EnterTransport {
                    passenger_id: gi,
                    transport_id: castle,
                },
            );
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
        assert!(matches!(
            sim.entities().get(gi).unwrap().passenger_role,
            PassengerRole::Inside { transport_id, .. } if transport_id == castle
        ));
    }
    let castle_entity = sim.entities().get(castle).unwrap();
    assert_eq!(castle_entity.health.current, castle_strength);
    assert_eq!(sim.resolve(castle_entity.owner()), owner_name);
}
