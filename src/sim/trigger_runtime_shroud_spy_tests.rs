//! Instruction-established Execute31/HasOccurred54 production regressions.
//! Steam gamemd SHA2563e81a617..08600; no whole-map/native rendering claim.
use super::tests::{event_ingress_fixture_with_rules, install_event_ingress_map};
use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::UninitContext;

fn fixture() -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    [InternedId; 3],
) {
    // The shared fixture appends this inside its TAGUNIT section. Supply
    // Sight there; a second [TAGUNIT]/[VehicleTypes] body is not a layer.
    event_ingress_fixture_with_rules(
        "Sight=1\n[General]\nAlliedDisguise=E1\nSovietDisguise=E2\nThirdDisguise=INIT\n\
         [InfantryTypes]\n2=SPY\n3=E1\n4=E2\n5=INIT\n\
         [SPY]\nStrength=100\nSpeed=4\nSpeedType=Foot\nAgent=yes\nInfiltrate=yes\nCanDisguise=yes\nPermaDisguise=yes\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [E1]\nStrength=100\n[E2]\nStrength=100\n[INIT]\nStrength=100\n",
    )
}

fn install(sim: &mut Simulation, rules: &RuleSet, value: i32) -> usize {
    install_event_ingress_map(
        sim,
        rules,
        &format!(
            "[Triggers]\nSPY=Country0,<none>,Spy,0,1,1,1,0\n\
         [Events]\nSPY=1,54,0,{value}\n\
         [Actions]\nSPY=1,28,0,7,0,0,0,0,A\n\
         [Tags]\nSPYTAG=0,Spy,SPY\n[CellTags]\n15015=SPYTAG\n"
        ),
    );
    let typ = sim.trigger_runtime.definitions.tag_type("SPYTAG").unwrap();
    let tag = sim.shared_trigger_tag(typ);
    sim.trigger_runtime.tag(tag).unwrap().triggers[0]
}

fn rng(sim: &Simulation) -> [String; 3] {
    [
        sim.scenario_rng.native_state_hex(),
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

#[test]
fn forced_defaults_and_makeup_kit_copy_use_the_same_retained_type_owner() {
    let (mut sim, rules, _, houses) = fixture();
    let actor = sim
        .spawn_object("SPY", "House0", 15, 15, 0, &rules)
        .unwrap();
    for (side, name) in [
        (Some(0), "E1"),
        (Some(1), "E2"),
        (Some(2), "INIT"),
        (Some(9), "INIT"),
        (None, "INIT"),
    ] {
        sim.session.current_house = side.map(|side| {
            sim.houses.get_mut(&houses[2]).unwrap().side_index = side;
            houses[2]
        });
        assert_eq!(
            sim.forced_disguise_type(actor, &rules),
            sim.interner.get(name)
        );
    }
    sim.session.current_house = Some(houses[1]);
    let target = sim
        .spawn_object("SPY", "House0", 16, 15, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .disguise
        .as_mut()
        .unwrap()
        .acquire(0, None, None);
    sim.disguise_as(actor, Some(target), true, &rules);
    let disguise = sim
        .substrate
        .entities
        .get(actor)
        .unwrap()
        .disguise
        .as_ref()
        .unwrap();
    assert_eq!(disguise.type_id(), sim.interner.get("E2"));
    assert_eq!(disguise.house(), Some(houses[1]));
}

#[test]
fn non_agent_arrival_does_not_raise_the_spy_cell_event() {
    let (mut sim, rules, registry, houses) = fixture();
    sim.session.current_house = Some(houses[0]);
    let actor = sim
        .spawn_object("ENGINEER", "House0", 15, 15, 0, &rules)
        .unwrap();
    let value = rules
        .type_array_index(crate::rules::object_type::ObjectCategory::Infantry, "E1")
        .unwrap();
    install(&mut sim, &rules, value);
    sim.per_cell_process(
        actor,
        crate::sim::movement::PerCellReason::Arrival,
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    )
    .unwrap();
    assert!(!sim.trigger_runtime.globals_set.contains(&7));
}

#[test]
fn spy_type_event_uses_forced_identity_side_fallback_and_native_persistence_gate() {
    use crate::map::entities::EntityCategory;
    use crate::rules::object_type::ObjectCategory;
    for (side, stored, raised, bypass, prior, expected) in [
        (Some(0), None, 54, false, false, true),
        (Some(1), None, 54, false, false, false),
        (Some(2), None, 54, false, false, false),
        (Some(9), None, 54, false, true, false),
        (None, None, 54, false, false, false),
        (Some(1), Some("E1"), 54, false, false, true),
        (Some(0), Some("E2"), 54, false, true, false),
        (Some(0), Some("E1"), 53, false, false, false),
        (Some(0), Some("E1"), 54, true, false, false),
    ] {
        let (mut sim, rules, _, houses) = fixture();
        sim.session.current_house = side.map(|side| {
            sim.houses.get_mut(&houses[2]).unwrap().side_index = side;
            houses[2]
        });
        let actor = sim
            .spawn_object("SPY", "House0", 15, 15, 0, &rules)
            .unwrap();
        let expected_index = rules
            .type_array_index(ObjectCategory::Infantry, "E1")
            .unwrap();
        let live = install(&mut sim, &rules, expected_index);
        if let Some(name) = stored {
            let typ = sim.interner.get(name).unwrap();
            let entity = sim.substrate.entities.get_mut(actor).unwrap();
            let disguise = entity.disguise.as_mut().unwrap();
            disguise.acquire(0, Some(typ), None);
            disguise.receive_damage_reveal(0, 1, EntityCategory::Infantry);
            assert!(!disguise.is_disguised(), "GetDisguise(1) ignores the flag");
        }
        let mut persistence = prior;
        let before = rng(&sim);
        let timer = sim.trigger_runtime.trigger(live).unwrap().timer;
        let mut call = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: raised,
                    actor: Some(actor),
                    ..Default::default()
                },
                None,
                &mut persistence,
                Some(&rules),
                live,
            )
        };
        let actual = if bypass {
            sim.with_buildup_bypass(call)
        } else {
            call(&mut sim)
        };
        assert_eq!(
            actual, expected,
            "side{side:?} stored{stored:?} raised{raised} bypass{bypass}"
        );
        assert_eq!(persistence, prior || expected);
        assert_eq!(rng(&sim), before);
        assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
        let mut persistence = false;
        assert!(!sim.has_occurred(
            0,
            TagEventInput {
                kind: 54,
                ..Default::default()
            },
            None,
            &mut persistence,
            Some(&rules),
            live
        ));
        let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[typ].events[0]
            .condition
            .value = -1;
        assert!(!sim.has_occurred(
            0,
            TagEventInput {
                kind: 54,
                actor: Some(actor),
                ..Default::default()
            },
            None,
            &mut persistence,
            Some(&rules),
            live
        ));
    }
}

#[test]
fn agent_arrival_delivers_cell_spy_event_after_snapshot_and_cleans_up_one_shot() {
    let (mut sim, rules, registry, houses) = fixture();
    sim.session.current_house = Some(houses[1]); // side1, not the spy owner's side0.
    let actor = sim
        .spawn_object("SPY", "House0", 15, 15, 0, &rules)
        .unwrap();
    let value = rules
        .type_array_index(crate::rules::object_type::ObjectCategory::Infantry, "E2")
        .unwrap();
    let live = install(&mut sim, &rules, value);
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Spy CellTag", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
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
            Some(&rules),
            Some(&registry),
            FrameEffects::default(),
        )
        .unwrap();
        assert!(!sim.trigger_runtime.globals_set.contains(&7));
        sim.per_cell_process(
            actor,
            crate::sim::movement::PerCellReason::Arrival,
            Some(&rules),
            Some(&registry),
            FrameEffects::default(),
        )
        .unwrap();
        assert!(sim.trigger_runtime.globals_set.contains(&7));
        assert_eq!(rng(sim), before);
        assert!(
            sim.substrate.entities.contains(actor),
            "crossing a CellTag does not consume the spy"
        );
        sim.flush_pending_delete();
        assert!(sim.trigger_runtime.trigger(live).is_none());
    }
}

#[test]
fn grow_action_uses_current_map_then_known_objects_without_resetting_house_flags() {
    let (mut sim, rules, registry, houses) = fixture();
    sim.fog = crate::sim::vision::FogState::new(33, 33);
    sim.session.current_house = Some(houses[0]);
    sim.session.game_mode_nonzero = true;
    let known = sim
        .spawn_object("TAGUNIT", "House0", 14, 15, 0, &rules)
        .unwrap();
    let unknown = sim
        .spawn_object("TAGUNIT", "House0", 23, 15, 0, &rules)
        .unwrap();
    let air = sim
        .spawn_object("TAGUNIT", "House0", 23, 23, 0, &rules)
        .unwrap();
    // Supply the AIR registration boundary. This check is about the map's
    // display cursor, not an emulated aircraft/locomotor transition.
    sim.submit_object_display(
        air,
        crate::sim::world::display_layers::DisplayLayer::AIR,
        Some(&rules),
    );
    sim.substrate
        .entities
        .get_mut(air)
        .unwrap()
        .discovery
        .set_discovered_by_current_house_for_test(true);
    sim.substrate
        .entities
        .get_mut(known)
        .unwrap()
        .discovery
        .set_discovered_by_current_house_for_test(true);
    sim.substrate
        .entities
        .get_mut(unknown)
        .unwrap()
        .discovery
        .set_discovered_by_current_house_for_test(false);
    for cell in [(14, 15), (23, 15), (23, 23), (8, 11)] {
        sim.fog
            .set_ground_bits_for_test(houses[0], cell.0, cell.1, 8);
    }
    sim.fog.set_ground_bits_for_test(houses[1], 8, 11, 24);
    sim.houses.get_mut(&houses[0]).unwrap().map_is_clear = true;
    let rng_before = rng(&sim);
    sim.trigger_grow_shroud(UninitContext::new(Some(&rules), Some(&registry)));
    assert!(sim.fog.is_ground_unshrouded(houses[0], 14, 15));
    assert!(!sim.fog.is_ground_unshrouded(houses[0], 23, 15));
    assert!(!sim.fog.is_ground_unshrouded(houses[0], 23, 23));
    assert!(!sim.fog.is_ground_unshrouded(houses[0], 8, 11));
    assert!(sim.fog.is_ground_open(houses[1], 8, 11));
    assert!(sim.houses[&houses[0]].map_is_clear);
    assert_eq!(rng(&sim), rng_before);
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nGROW=Country7,<none>,Grow,0,1,1,1,0\n[Events]\nGROW=1,8,0,0\n[Actions]\nGROW=2,31,0,0,0,0,0,0,A,28,0,7,0,0,0,0,A\n[Tags]\nGROWTAG=2,Grow,GROW\n",
    );
    sim.fog.set_ground_bits_for_test(houses[0], 8, 11, 8);
    sim.session.binary_frame = 8;
    sim.raise_house_tags(
        houses[1],
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(!sim.fog.is_ground_unshrouded(houses[0], 8, 11));
    assert!(sim.trigger_runtime.globals_set.contains(&7));
}
