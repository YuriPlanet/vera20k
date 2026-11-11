use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::retail_ini_fixture::{art_registry, retail_assets, retail_rules_and_art};
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::DriveCoord;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{FrameEffects, Simulation, TickLane};

fn retail_trailer_world(trailer: &str, delay: i32) -> Option<(Simulation, RuleSet, u64)> {
    let (root, mut art) = retail_rules_and_art()?;
    art.merge(&IniFile::from_str(&format!(
        "[PDPLANE]\nTrailer={trailer}\nSpawnDelay={delay}\n"
    )));
    let mut layers = RulesLayerStack::new(root);
    // A normal later pass supplies ART for any late-allocated Trailer slot.
    layers.push(RulesLayerKind::Scenario, IniFile::empty());
    let processed = layers.process_with_fixed_art(&art).unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.install_art_data(art_registry(&art));
    let (_, mut assets) = retail_assets().expect("retail animation assets");
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    assert_eq!(
        rules.bind_anim_class_assets(&["SMKPUFF".into()], &assets, "TEM", "TEMPERATE"),
        0
    );
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    sim.install_overlay_grid_for_load(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    sim.session.set_binary_frame_for_test(103);
    let plane = sim
        .spawn_object_at_height("PDPLANE", "Americans", 12, 12, 0, 0, &rules)
        .expect("retail aircraft constructor");
    sim.substrate
        .mission_assign_exact(plane, MissionId::from_known(MissionType::Sleep), 103)
        .unwrap();
    sim.substrate.mission_commence_exact(plane, 103).unwrap();
    sim.substrate
        .entities
        .get_mut(plane)
        .unwrap()
        .position
        .put_location(DriveCoord {
            x: 12 * 256 + 47,
            y: 12 * 256 + 93,
            z: 777,
        });
    assert!(sim.substrate.entities.get(plane).unwrap().is_mission_only());
    Some((sim, rules, plane))
}

/// AircraftAI414ED0..414F42 emits an independent delayed Anim at the raw
/// post-Foot Location, before the off-map removal at414F47. Cadence uses the
/// global frame, not the aircraft's age; the ordinary Anim owns its lifetime.
#[test]
fn authored_retail_trailer_keeps_global_cadence_and_outlives_its_aircraft() {
    let Some((mut sim, rules, plane)) = retail_trailer_world("SMKPUFF", 2) else {
        return;
    };
    let registry = rules.overlay_registry().clone();

    assert!(
        sim.advance_tick(&[], Some(&rules), None, None, 67)
            .frame_committed
    );
    assert_eq!(sim.session.binary_frame(), 104);
    assert!(sim.substrate.entities.contains(plane));
    // Frame103 was not due. The next,104, differs from the default3 cadence.
    assert_eq!(sim.anims().count(), 0);
    sim.advance_tick(&[], Some(&rules), None, None, 67); // Frame104: due, unlike default3.
    let (&first_id, first) = sim.anims().next().expect("due aircraft emits its trailer");
    assert_eq!(sim.anims().count(), 1);
    assert_eq!(sim.interner.resolve(first.type_id()), "SMKPUFF");
    assert_eq!(
        first.world_coord(),
        AnimWorldCoord::from(position_world_coord(
            &sim.substrate.entities.get(plane).unwrap().position,
        ))
    );
    assert!(first.world_coord().z > 0);
    assert_eq!(first.draw_flags(), 0x600);
    assert_eq!(first.runtime().delay_remaining, 1);
    assert_eq!(first.runtime().loop_remaining, 1);
    assert_eq!((first.owner_entity(), first.owner_house()), (None, None));

    // Preserve the still-delayed child through the normal snapshot restore.
    let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
    let saved = GameSnapshot::save(&sim, 0, 0, "aircraft trailer", 0);
    let mut sim = GameSnapshot::load(&saved).unwrap().sim;
    sim.restore_after_snapshot_load().unwrap();
    sim.rebuild_caches_after_load(terrain, Default::default(), &rules);
    sim.restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .unwrap();
    assert_eq!(sim.anim(first_id).unwrap().runtime().delay_remaining, 1);
    sim.advance_tick(&[], Some(&rules), None, None, 67); // Frame105: child Start.
    assert_eq!(sim.anims().count(), 1, "adjacent frame must not emit");
    assert_eq!(sim.anim(first_id).unwrap().runtime().delay_remaining, 0);

    // Final AI-entry pose is outside Size. PDPLANE's constructor supplied
    // mission-only, and its earlier inside visit supplied the playfield latch.
    assert!(sim.substrate.entities.get(plane).unwrap().in_playfield());
    sim.substrate
        .entities
        .get_mut(plane)
        .unwrap()
        .position
        .put_location(DriveCoord {
            x: -512 + 47,
            y: 12 * 256 + 93,
            z: 777,
        });
    sim.advance_tick(&[], Some(&rules), None, None, 67); // Frame106: emit, then remove.
    assert!(!sim.substrate.entities.contains(plane));
    let (&last_id, last) = sim
        .anims()
        .find(|(id, _)| **id != first_id)
        .expect("off-map removal follows the final due trailer");
    assert!(last.world_coord().x < 0);
    assert_eq!(last.runtime().delay_remaining, 1);
    assert_eq!((last.owner_entity(), last.owner_house()), (None, None));
    for _ in 0..512 {
        if sim.anim(first_id).is_none() && sim.anim(last_id).is_none() {
            break;
        }
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    assert!(sim.anim(first_id).is_none() && sim.anim(last_id).is_none());
}

/// AircraftAI414EFC skips division for null Trailer. Reached IDIV414F06
/// rejects zero and signed overflow before Anim421EA0 spends an ID or RNG.
#[test]
fn trailer_invalid_divisors_stop_before_construction_and_fail_the_frame() {
    let Some((mut sim, rules, plane)) = retail_trailer_world("none", 0) else {
        return;
    };
    let constructor_state = |sim: &Simulation| {
        (
            sim.scenario_rng.native_state_hex(),
            sim.native_unique_ids.as_ref().map(|ids| ids.current_raw()),
            sim.anims().count(),
        )
    };
    let before = constructor_state(&sim);
    sim.emit_aircraft_trailer(plane, &rules, FrameEffects::default())
        .expect("null Trailer bypasses division by zero");
    assert_eq!(constructor_state(&sim), before);

    let (mut sim, rules, plane) = retail_trailer_world("SMKPUFF", 0).unwrap();
    let before = constructor_state(&sim);
    assert!(
        sim.emit_aircraft_trailer(plane, &rules, FrameEffects::default())
            .is_err()
    );
    assert_eq!(constructor_state(&sim), before);
    let error = sim
        .advance_app_frame(
            &[],
            &rules,
            None,
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .expect_err("reached invalid cadence must fail the production frame");
    assert_eq!((error.entity_id, error.binary_frame), (plane, 103));
    assert!(error.cause.contains("SpawnDelay 0"));
    assert_eq!(sim.anims().count(), 0);

    let (mut sim, rules, plane) = retail_trailer_world("SMKPUFF", -1).unwrap();
    sim.session.set_binary_frame_for_test(0x8000_0000);
    let before = constructor_state(&sim);
    assert!(
        sim.emit_aircraft_trailer(plane, &rules, FrameEffects::default())
            .is_err()
    );
    assert_eq!(constructor_state(&sim), before);
}
