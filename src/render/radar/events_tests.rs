use super::*;

const TYPE5_BRIGHT: [u8; 4] = [0, 255, 255, 255];

/// Stock `rulesmd.ini` `[General]` radar-event values.
fn configured() -> RadarEventConfig {
    RadarEventConfig::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[General]\nRadarEventMinRadius=8\nRadarEventSpeed=1.2\n\
         RadarEventRotationSpeed=.05\nRadarEventColorSpeed=.1\n",
    ))
}

fn queue_ready() -> ClientRadarEvents {
    let mut queue = ClientRadarEvents::default();
    queue.finish_baseline();
    queue
}

fn source(cell: (u16, u16), pixel: (i32, i32)) -> RadarEventSource {
    RadarEventSource {
        cell,
        radar_pixel: pixel,
    }
}

#[test]
fn terrain_rebuild_preserves_live_alert_and_restarts_sighting_baseline() {
    let mut queue = queue_ready();
    let config = configured();
    let origin = source((20, 20), (40, 40));
    assert!(queue.create(
        RadarEventType::BaseUnderAttack,
        origin,
        10,
        (140, 108),
        &config
    ));
    queue.advance_to_frame(20, &config);
    queue.restart_tracker_baseline();
    assert_eq!(
        queue.len(),
        1,
        "RadarInit655B20 does not delete saved alerts"
    );
    assert!(!queue.create_enemy_sensed(origin, 20, (140, 108), &config));
    assert!(!queue.create(
        RadarEventType::BaseUnderAttack,
        origin,
        20,
        (140, 108),
        &config
    ));
}

#[test]
fn saved_alerts_round_trip_without_animation_and_clear_postload_review_ring() {
    let saved = saved_radar_fixture();
    let bytes = crate::util::save_codec::serialize(&saved).unwrap();
    let decoded: SavedRadarEvents = crate::util::save_codec::deserialize(&bytes).unwrap();
    let mut restored = ClientRadarEvents::from_saved(decoded).unwrap();
    assert_eq!(restored.saved(), saved);
    assert_eq!(restored.cycle_cell(Instant::now()), None);
    assert!(!restored.create_enemy_sensed(source((1, 1), (1, 1)), 100, (140, 108), &configured()));
    restored.advance_to_frame(100, &configured());
    assert_eq!(
        restored.saved(),
        saved,
        "the saved frame is not drawn twice"
    );
}

#[test]
fn load_keeps_invisible_dedup_and_resumes_the_next_animation_frame() {
    let mut original = queue_ready();
    let config = configured();
    let origin = source((20, 20), (40, 40));
    assert!(original.create(
        RadarEventType::BaseUnderAttack,
        origin,
        0,
        (140, 108),
        &config
    ));
    // Start at the same completed-shrink boundary as the existing lifetime
    // control; this test concerns storage of the ensuing invisible interval.
    original.events[0].radius = config.min_radius as f32;
    original.events[0].rotation_speed = 1.0;
    original.advance_to_frame(0, &config);
    original.advance_to_frame(300, &config);
    assert!(!original.events[0].needs_draw);
    assert!(original.create_enemy_sensed(source((50, 50), (70, 70)), 300, (140, 108), &config));
    original.advance_to_frame(310, &config);
    let mut restored = ClientRadarEvents::from_saved(original.saved()).unwrap();
    assert!(!restored.create(
        RadarEventType::BaseUnderAttack,
        origin,
        310,
        (140, 108),
        &config
    ));
    assert_eq!(restored.cycle_cell(Instant::now()), None);
    restored.finish_baseline();
    original.advance_to_frame(311, &config);
    restored.advance_to_frame(311, &config);
    assert_eq!(original.saved(), restored.saved());
    original.advance_to_frame(600, &config);
    restored.advance_to_frame(600, &config);
    assert_eq!(original.saved(), restored.saved());
    assert!(restored.create(
        RadarEventType::BaseUnderAttack,
        origin,
        600,
        (140, 108),
        &config
    ));
}

#[test]
fn malformed_saved_animation_is_rejected_before_installation() {
    let mut saved = saved_radar_fixture();
    saved.events[0].rotation = f32::NAN.to_bits();
    assert!(matches!(
        ClientRadarEvents::from_saved(saved),
        Err(SavedRadarEventsError::NonFiniteAnimation { index: 0 })
    ));
}

#[test]
fn type5_dedup_is_strictly_less_than_six_and_ring_accepts_only_created_events() {
    let mut queue = queue_ready();
    let config = configured();
    assert!(queue.create_enemy_sensed(source((10, 10), (20, 20)), 1, (200, 200), &config));
    assert!(!queue.create_enemy_sensed(source((15, 10), (25, 20)), 1, (200, 200), &config));
    assert!(queue.create_enemy_sensed(source((16, 10), (26, 20)), 1, (200, 200), &config));
    assert_eq!(queue.len(), 2);

    let now = Instant::now();
    assert_eq!(queue.cycle_cell(now), Some((16, 10)));
    assert_eq!(
        queue.cycle_cell(now + Duration::from_millis(1)),
        Some((10, 10))
    );
}

#[test]
fn type5_initial_radius_uses_farthest_surface_edge() {
    let mut queue = queue_ready();
    let config = configured();
    assert!(queue.create_enemy_sensed(source((1, 1), (2, 3)), 7, (200, 120), &config));
    assert_eq!(queue.events[0].radius, 198.0);
}

#[test]
fn type5_phase_boundaries_keep_live_dedup_after_draw_stops_then_expire() {
    let mut queue = queue_ready();
    let config = configured();
    assert!(queue.create_enemy_sensed(source((20, 20), (100, 60)), 0, (200, 120), &config));
    let event = &mut queue.events[0];
    event.radius = config.min_radius as f32;
    event.rotation_speed = 1.0;
    event.tick(0, &config);
    assert!(!event.expanding);
    assert!(event.needs_draw);

    queue.advance_to_frame(199, &config);
    assert!(queue.events[0].needs_draw);
    queue.advance_to_frame(200, &config);
    assert!(!queue.events[0].needs_draw);
    let stopped_fade = queue.events[0].fade;
    queue.advance_to_frame(201, &config);
    assert_eq!(
        queue.events[0].fade, stopped_fade,
        "+0x3D false makes the following TickRadarEvent visit an immediate no-op"
    );
    assert_eq!(
        queue.len(),
        1,
        "invisible event still owns unique suppression"
    );
    queue.advance_to_frame(399, &config);
    assert_eq!(queue.len(), 1);
    queue.advance_to_frame(400, &config);
    assert_eq!(queue.len(), 0);
    assert!(queue.create_enemy_sensed(source((20, 20), (100, 60)), 401, (200, 120), &config));
}

#[test]
fn type5_rotation_remainder_and_deceleration_follow_x87_stores() {
    assert_eq!(
        native_rotation_remainder(f32::from_bits(0x3f49_0fdb)).to_bits(),
        0x32bb_bd2e,
        "0x65FE69 computes and stores the pi/4-aligned fractional remainder"
    );
    let base = configured().native_scalars.rotation_speed;
    let first = native_decelerated_rotation_speed(0.05, base);
    assert_eq!(first.to_bits(), 0x3d48_b439);
}

#[test]
fn type5_fade_bounces_at_zero_and_one() {
    let mut event = ClientRadarEvent::new(
        RadarEventType::EnemyObjectSensed,
        source((1, 1), (5, 5)),
        0,
        (10, 10),
        &configured(),
    );
    event.fade = 0.95;
    event.fade_speed = 0.1;
    event.tick_fade();
    assert_eq!(event.fade, 1.0);
    assert!(event.fade_speed < 0.0);
    event.fade = 0.05;
    event.fade_speed = -0.1;
    event.tick_fade();
    assert_eq!(event.fade, 0.0);
    assert!(event.fade_speed > 0.0);
}

#[test]
fn type5_draw_is_one_cyan_outline_in_insertion_order() {
    let mut queue = queue_ready();
    let config = configured();
    assert!(queue.create_enemy_sensed(source((1, 1), (12, 12)), 0, (24, 24), &config));
    let event = &mut queue.events[0];
    event.radius = 4.0;
    event.rotation = 0.0;
    event.fade = 1.0;
    event.expanding = false;
    let mut rgba = vec![7_u8; 24 * 24 * 4];
    queue.draw(&mut rgba, 24, 24, (24, 24), (0, 0));
    let pixel = |x: usize, y: usize| {
        let offset = (y * 24 + x) * 4;
        &rgba[offset..offset + 4]
    };
    assert_eq!(pixel(16, 12), TYPE5_BRIGHT);
    assert_eq!(pixel(12, 12), [7, 7, 7, 7], "no fill or inner diamond");
}

#[test]
fn type5_draw_clips_to_generated_primary_extent_not_host_texture() {
    let mut queue = queue_ready();
    let config = configured();
    assert!(queue.create_enemy_sensed(source((1, 1), (138, 40)), 0, (140, 84), &config));
    let event = &mut queue.events[0];
    event.radius = 20.0;
    event.rotation = 0.0;
    event.fade = 1.0;
    event.expanding = false;
    let mut rgba = vec![7_u8; 200 * 200 * 4];
    queue.draw(&mut rgba, 200, 200, (140, 84), (0, 0));
    let pixel = |x: usize, y: usize| {
        let offset = (y * 200 + x) * 4;
        &rgba[offset..offset + 4]
    };
    assert_eq!(pixel(139, 21), TYPE5_BRIGHT);
    assert_eq!(pixel(140, 22), [7, 7, 7, 7]);
    assert_eq!(pixel(158, 40), [7, 7, 7, 7]);
}

#[test]
fn type5_draw_applies_only_the_centered_primary_surface_copy() {
    let mut queue = queue_ready();
    let config = configured();
    assert!(queue.create_enemy_sensed(source((1, 1), (10, 10)), 0, (64, 108), &config));
    let event = &mut queue.events[0];
    event.radius = 4.0;
    event.rotation = 0.0;
    event.fade = 1.0;
    event.expanding = false;
    let mut rgba = vec![7_u8; 140 * 108 * 4];
    queue.draw(&mut rgba, 140, 108, (64, 108), (38, 0));
    let pixel = |x: usize, y: usize| {
        let offset = (y * 140 + x) * 4;
        &rgba[offset..offset + 4]
    };
    assert_eq!(pixel(52, 10), TYPE5_BRIGHT);
    assert_eq!(pixel(14, 10), [7, 7, 7, 7], "no untransformed event copy");
}

#[test]
fn type5_cycle_ring_wraps_and_restarts_at_newest() {
    let mut queue = queue_ready();
    let config = configured();
    for cell in 0..10_u16 {
        assert!(queue.create_enemy_sensed(
            source((cell * 6, 0), (i32::from(cell), 0)),
            0,
            (200, 200),
            &config,
        ));
    }
    let now = Instant::now();
    assert_eq!(queue.cycle_cell(now), Some((54, 0)));
    let expected = [
        (48, 0),
        (42, 0),
        (36, 0),
        (30, 0),
        (24, 0),
        (18, 0),
        (12, 0),
        (54, 0),
    ];
    for (step, cell) in expected.into_iter().enumerate() {
        assert_eq!(
            queue.cycle_cell(now + Duration::from_millis(step as u64 + 1)),
            Some(cell)
        );
    }
    assert_eq!(
        queue.cycle_cell(now + CYCLE_RESTART + Duration::from_millis(20)),
        Some((54, 0))
    );
}

#[test]
fn view_change_discards_previous_client_events_and_sighting_baseline() {
    let mut queue = ClientRadarEvents::default();
    let config = configured();
    assert!(!queue.create_enemy_sensed(source((4, 4), (8, 8)), 10, (20, 20), &config));
    assert_eq!(queue.len(), 0);
    queue.finish_baseline();
    assert!(queue.create_enemy_sensed(source((4, 4), (8, 8)), 11, (20, 20), &config));
    queue.reset_for_view_change();
    assert_eq!(queue.len(), 0);
    assert!(!queue.create_enemy_sensed(source((4, 4), (8, 8)), 12, (20, 20), &config));
}

#[test]
fn client_radar_events_cannot_change_world_hash() {
    let sim = crate::sim::world::Simulation::new();
    let before = sim.state_hash();
    let mut queue = queue_ready();
    assert!(queue.create_enemy_sensed(source((12, 18), (40, 80)), 1, (200, 200), &configured(),));
    queue.advance_to_frame(5, &configured());
    assert_eq!(sim.state_hash(), before);
}

#[test]
fn compiled_event_rows_match_gamemd() {
    // Direct read of `g_RadarEventTypeConfig @ 0x007F0998` (17 x 16 bytes):
    // {dedup cells, visibility frames, lifetime frames, unique}.
    let expected = [
        (8, 200, 400, true),
        (8, 200, 400, false),
        (8, 200, 400, false),
        (8, 200, 600, true),
        (8, 200, 400, true),
        (6, 200, 400, true),
        (2, 0, 200, true),
        (8, 0, 200, true),
        (2, 0, 400, true),
        (5, 0, 400, false),
        (8, 0, 100, false),
        (8, 200, 200, true),
        (8, 200, 400, false),
        (8, 0, 5, false),
        (8, 0, 200, true),
        (8, 0, 400, true),
        (8, 200, 600, true),
    ];
    for (row, (dedup, visible, lifetime, unique)) in RADAR_EVENT_TYPE_CONFIGS.iter().zip(expected) {
        assert_eq!(row.dedup_distance_cells, dedup);
        assert_eq!(row.visible_frames, visible);
        assert_eq!(row.lifetime_frames, lifetime);
        assert_eq!(row.unique, unique);
    }
    assert_eq!(
        config_for(RadarEventType::AllyUnderAttack),
        RADAR_EVENT_TYPE_CONFIGS[16]
    );
}

#[test]
fn only_native_drawing_types_have_outline_colors() {
    let white = ([255, 255, 255, 255], [128, 128, 128, 255]);
    let yellow = ([255, 255, 0, 255], [128, 128, 0, 255]);
    let cyan = ([0, 255, 255, 255], [0, 128, 128, 255]);
    for (event_type, colors) in [
        (RadarEventType::Combat, white),
        (RadarEventType::Noncombat, yellow),
        (RadarEventType::Dropzone, yellow),
        (RadarEventType::BaseUnderAttack, white),
        (RadarEventType::HarvesterUnderAttack, white),
        (RadarEventType::EnemyObjectSensed, cyan),
        (RadarEventType::BeaconPlaced, yellow),
        (RadarEventType::ConstructionComplete, yellow),
    ] {
        assert_eq!(outline_colors(event_type), Some(colors));
    }
    for event_type in [
        RadarEventType::UnitReady,
        RadarEventType::UnitLost,
        RadarEventType::UnitRepaired,
        RadarEventType::SpyInfiltration,
        RadarEventType::BuildingCaptured,
        RadarEventType::ImpactSilent,
        RadarEventType::BridgeRepaired,
        RadarEventType::StructureAbandoned,
        RadarEventType::AllyUnderAttack,
    ] {
        assert_eq!(outline_colors(event_type), None);
    }
}

#[test]
fn dedup_is_per_type_with_its_own_distance_and_skips_non_unique_types() {
    let mut queue = ClientRadarEvents::default();
    let config = configured();
    let at = |x: u16| source((x, 10), (i32::from(x), 10));
    // Sim-published types are not subject to the type-5 tracker baseline.
    assert!(queue.create(RadarEventType::UnitLost, at(10), 0, (200, 200), &config));
    assert!(!queue.create(RadarEventType::UnitLost, at(17), 0, (200, 200), &config));
    assert!(queue.create(RadarEventType::UnitLost, at(18), 0, (200, 200), &config));
    // Another type at the same cell is not a duplicate.
    assert!(queue.create(
        RadarEventType::BaseUnderAttack,
        at(10),
        0,
        (200, 200),
        &config
    ));
    // UnitReady dedupes inside two cells only.
    assert!(queue.create(RadarEventType::UnitReady, at(10), 0, (200, 200), &config));
    assert!(!queue.create(RadarEventType::UnitReady, at(11), 0, (200, 200), &config));
    assert!(queue.create(RadarEventType::UnitReady, at(12), 0, (200, 200), &config));
    // BuildingCaptured is not unique: every call creates an event.
    assert!(queue.create(
        RadarEventType::BuildingCaptured,
        at(10),
        0,
        (200, 200),
        &config
    ));
    assert!(queue.create(
        RadarEventType::BuildingCaptured,
        at(10),
        0,
        (200, 200),
        &config
    ));
}

#[test]
fn non_drawing_event_never_paints_and_dedupes_for_its_own_lifetime() {
    let mut queue = ClientRadarEvents::default();
    let config = configured();
    assert!(queue.create(
        RadarEventType::UnitLost,
        source((20, 20), (12, 12)),
        0,
        (24, 24),
        &config
    ));
    let event = &mut queue.events[0];
    event.radius = config.min_radius as f32;
    event.rotation_speed = 1.0;
    event.tick(0, &config);
    assert!(!event.expanding);
    // Visibility 0: the very next tick clears +0x3D.
    queue.advance_to_frame(1, &config);
    assert!(!queue.events[0].needs_draw);

    let mut rgba = vec![7_u8; 24 * 24 * 4];
    queue.events[0].needs_draw = true;
    queue.draw(&mut rgba, 24, 24, (24, 24), (0, 0));
    assert!(
        rgba.iter().all(|&byte| byte == 7),
        "default colour arm draws nothing"
    );
    queue.events[0].needs_draw = false;

    queue.advance_to_frame(199, &config);
    assert!(!queue.create(
        RadarEventType::UnitLost,
        source((21, 20), (12, 12)),
        199,
        (24, 24),
        &config
    ));
    queue.advance_to_frame(200, &config);
    assert_eq!(
        queue.len(),
        0,
        "type 7 lifetime is 200 frames past the shrink phase"
    );
    assert!(queue.create(
        RadarEventType::UnitLost,
        source((21, 20), (12, 12)),
        201,
        (24, 24),
        &config
    ));
}

#[test]
fn review_ring_holds_every_accepted_type_in_creation_order() {
    // The former split kept sim-produced events in a second history that
    // Spacebar could not reach once a type-5 cell existed.
    let mut queue = queue_ready();
    let config = configured();
    assert!(queue.create_enemy_sensed(source((5, 5), (5, 5)), 0, (200, 200), &config));
    assert!(queue.create(
        RadarEventType::UnitLost,
        source((40, 40), (40, 40)),
        1,
        (200, 200),
        &config
    ));
    assert!(queue.create(
        RadarEventType::BaseUnderAttack,
        source((70, 70), (70, 70)),
        2,
        (200, 200),
        &config
    ));
    // A suppressed duplicate does not enter the ring.
    assert!(!queue.create(
        RadarEventType::BaseUnderAttack,
        source((71, 70), (71, 70)),
        2,
        (200, 200),
        &config
    ));
    let now = Instant::now();
    assert_eq!(queue.cycle_cell(now), Some((70, 70)));
    assert_eq!(
        queue.cycle_cell(now + Duration::from_millis(1)),
        Some((40, 40))
    );
    assert_eq!(
        queue.cycle_cell(now + Duration::from_millis(2)),
        Some((5, 5))
    );
}

#[test]
fn reinforcement_cells_share_review_order_without_creating_or_deduping_events() {
    let mut queue = queue_ready();
    let config = configured();
    assert!(queue.create(
        RadarEventType::BaseUnderAttack,
        source((70, 70), (70, 70)),
        0,
        (200, 200),
        &config,
    ));
    queue.remember_event_cell((70, 70));
    queue.remember_event_cell((71, 70));
    assert_eq!(
        queue.len(),
        1,
        "remembering a cell does not admit an outlined event"
    );
    let now = Instant::now();
    assert_eq!(queue.cycle_cell(now), Some((71, 70)));
    assert_eq!(
        queue.cycle_cell(now + Duration::from_millis(1)),
        Some((70, 70))
    );
    assert_eq!(
        queue.cycle_cell(now + Duration::from_millis(2)),
        Some((70, 70))
    );

    // A successful arrival writes every cell, including repeats; eight-cell
    // retention is independent of live radar-event uniqueness and cleanup.
    for cell in 0..10_i16 {
        queue.remember_event_cell((cell, 20));
    }
    queue.events.clear();
    let restart = now + CYCLE_RESTART + Duration::from_millis(10);
    for (step, cell) in (2..10_u16).rev().enumerate() {
        assert_eq!(
            queue.cycle_cell(restart + Duration::from_millis(step as u64)),
            Some((cell, 20)),
        );
    }
    queue.reset_for_view_change();
    assert_eq!(queue.cycle_cell(restart), None);
}

fn request(event_type: RadarEventType, rx: u16, ry: u16) -> crate::sim::radar::RadarEventRequest {
    crate::sim::radar::RadarEventRequest::new(event_type, rx, ry)
}

#[test]
fn admit_ages_by_frame_without_a_composed_radar_frame_until_the_dedupe_expires() {
    // No `advance_to_frame` call here: the app may admit many requests
    // between two radar frames, and an expired event must not keep deduping.
    let mut queue = ClientRadarEvents::default();
    let config = configured();
    let geometry = Some(((60, 40), (200, 120)));
    assert!(queue.admit(
        request(RadarEventType::UnitLost, 20, 20),
        0,
        geometry,
        &config
    ));
    assert!(!queue.admit(
        request(RadarEventType::UnitLost, 22, 20),
        0,
        geometry,
        &config
    ));

    let mut readmitted_at = None;
    for frame in 1..2_000_u64 {
        if queue.admit(
            request(RadarEventType::UnitLost, 22, 20),
            frame,
            geometry,
            &config,
        ) {
            readmitted_at = Some(frame);
            break;
        }
    }
    let readmitted_at = readmitted_at.expect("type 7 dedupe ends with its event");
    // Initial radius is the farthest surface edge (140 px); at 1.2 px/frame the
    // shrink alone outlasts 100 frames, and the 200-frame lifetime starts only
    // after it. A fixed 200-frame window, as the former sim queue used, would
    // have re-admitted far earlier.
    assert!(readmitted_at > 300, "re-admitted at frame {readmitted_at}");
    assert_eq!(queue.len(), 1, "only the re-admitted event is left");

    // The catch-up stops one frame short, so a later radar frame for the same
    // frame neither skips nor repeats the creation-frame tick.
    let before = queue.events[0].radius;
    queue.advance_to_frame(readmitted_at, &config);
    let after_first = queue.events[0].radius;
    assert!(after_first < before, "creation-frame tick ran once");
    queue.advance_to_frame(readmitted_at, &config);
    assert_eq!(queue.events[0].radius, after_first, "and only once");
}

#[test]
fn admit_without_radar_geometry_still_gates_by_cell_distance() {
    let mut queue = ClientRadarEvents::default();
    let config = configured();
    assert!(queue.admit(request(RadarEventType::UnitReady, 10, 10), 5, None, &config));
    assert!(!queue.admit(request(RadarEventType::UnitReady, 11, 10), 5, None, &config));
    assert!(queue.admit(request(RadarEventType::UnitReady, 12, 10), 5, None, &config));
    assert_eq!(
        queue.events[0].radius, 0.0,
        "no surface: no shrink distance"
    );
}

/// Original initialized kind6: whole65FDD0 -> 65FE00, then whole6603B0
/// at every supplied frame0..200. This compares the retained zero-geometry
/// prior and all its phase/scalar/lifetime results, not stock radar pixels.
#[test]
fn native_unit_ready_marker_matches_every_original_tick_and_cleanup() {
    use crate::util::read_helpers::{read_i32_le, read_u16_le, read_u32_le};

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let fixture = crate::rules::retail_ini_fixture::factory_unit_ready_native();
    let native = &fixture["controls"]["radar"];
    let decode_hex = |value: &serde_json::Value| {
        let hex = value.as_str().unwrap();
        (0..hex.len())
            .step_by(2)
            .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
            .collect::<Vec<_>>()
    };
    let config = &retail.rules.radar_event_config;
    let scalars = decode_hex(&native["native_scalar_bytes"]);
    assert_eq!(config.min_radius, read_i32_le(&scalars, 4));
    assert_eq!(config.color_speed.to_bits(), read_u32_le(&scalars, 0));
    assert_eq!(config.speed.to_bits(), read_u32_le(&scalars, 8));
    assert_eq!(config.rotation_speed.to_bits(), read_u32_le(&scalars, 12));
    let type6 = &native["native_type6_fields"];
    let row = config_for(RadarEventType::UnitReady);
    assert_eq!(
        i64::from(row.dedup_distance_cells),
        type6[0].as_i64().unwrap()
    );
    assert_eq!(u64::from(row.visible_frames), type6[1].as_u64().unwrap());
    assert_eq!(u64::from(row.lifetime_frames), type6[2].as_u64().unwrap());
    assert_eq!(row.unique, type6[3].as_u64().unwrap() != 0);

    let compare_event = |event: &ClientRadarEvent, raw: &[u8], context: &str| {
        assert_eq!(raw.len(), 64);
        assert_eq!(
            event.event_type as u32,
            read_u32_le(raw, 0),
            "{context}: kind"
        );
        assert_eq!(
            event.source.radar_pixel,
            (read_i32_le(raw, 4), read_i32_le(raw, 8))
        );
        assert_eq!(
            event.radius.to_bits(),
            read_u32_le(raw, 0x0c),
            "{context}: radius"
        );
        assert_eq!(
            event.rotation.to_bits(),
            read_u32_le(raw, 0x10),
            "{context}: rotation"
        );
        assert_eq!(
            event.rotation_speed.to_bits(),
            read_u32_le(raw, 0x14),
            "{context}: rotation speed"
        );
        assert_eq!(
            event.fade.to_bits(),
            read_u32_le(raw, 0x18),
            "{context}: fade"
        );
        assert_eq!(
            event.fade_speed.to_bits(),
            read_u32_le(raw, 0x1c),
            "{context}: fade speed"
        );
        assert_eq!(
            event.source.cell,
            (read_u16_le(raw, 0x20), read_u16_le(raw, 0x22))
        );
        assert_eq!(event.expanding, raw[0x3c] != 0, "{context}: shrink phase");
        assert_eq!(
            event.needs_draw,
            raw[0x3d] != 0,
            "{context}: drawing active"
        );
        assert_eq!(
            event.phase_started_frame,
            (!event.expanding).then(|| u64::from(read_u32_le(raw, 0x24))),
            "{context}: native lifetime start"
        );
    };
    let birth = decode_hex(&native["entry_birth_bytes"]);
    let cell = (read_u16_le(&birth, 0x20), read_u16_le(&birth, 0x22));
    let mut queue = ClientRadarEvents::default();
    assert!(queue.create(
        RadarEventType::UnitReady,
        source(cell, (read_i32_le(&birth, 4), read_i32_le(&birth, 8))),
        0,
        (0, 0), // Explicit inherited native surface prior.
        config,
    ));
    compare_event(&queue.events[0], &birth, "original kind6 birth");
    assert!(!queue.create(
        RadarEventType::UnitReady,
        source(cell, (0, 0)),
        0,
        (0, 0),
        config
    ));

    let frames = native["frames"].as_array().unwrap();
    assert_eq!(
        frames.len() as u64,
        native["expiry_frame"].as_u64().unwrap() + 1
    );
    for (index, frame) in frames.iter().enumerate() {
        let now = frame["frame"].as_u64().unwrap();
        assert_eq!(
            now, index as u64,
            "every original consecutive visit retained"
        );
        queue.advance_to_frame(now, config);
        let expected = &frame["cleanup_after_radar"];
        assert_eq!(
            queue.len() as u64,
            expected["count"].as_u64().unwrap(),
            "frame{now}: native cleanup count"
        );
        if let Some(event) = queue.events.first() {
            compare_event(
                event,
                &decode_hex(&expected["entries"][0]["raw_hex"]),
                &format!("original frame{now}"),
            );
        }
    }
    assert!(queue.events.is_empty());
    assert_eq!(
        queue.cycle_cell(Instant::now()),
        Some(cell),
        "native review ring survives expiry"
    );
}
