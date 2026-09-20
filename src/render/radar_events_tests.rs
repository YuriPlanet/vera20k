use super::*;

const TYPE5_BRIGHT: [u8; 4] = [0, 255, 255, 255];

fn configured() -> RadarEventConfig {
    RadarEventConfig::default()
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
    event.radius = config.min_radius;
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
fn stale_rebuild_baseline_cannot_replay_enemy_sensed() {
    let mut queue = ClientRadarEvents::default();
    let config = configured();
    assert!(!queue.create_enemy_sensed(source((4, 4), (8, 8)), 10, (20, 20), &config));
    assert_eq!(queue.len(), 0);
    queue.finish_baseline();
    assert!(queue.create_enemy_sensed(source((4, 4), (8, 8)), 11, (20, 20), &config));
    queue.reset_for_load_or_view();
    assert_eq!(queue.len(), 0);
    assert!(!queue.create_enemy_sensed(source((4, 4), (8, 8)), 12, (20, 20), &config));
}

#[test]
fn ordinary_and_repeated_action40_feeds_share_one_type5_dedup_authority() {
    let mut queue = queue_ready();
    let config = configured();
    let sensed = source((30, 30), (90, 90));
    assert!(queue.create_enemy_sensed(sensed, 50, (200, 200), &config));
    assert!(!queue.create_enemy_sensed(sensed, 50, (200, 200), &config));
    assert!(!queue.create_enemy_sensed(sensed, 51, (200, 200), &config));
    assert_eq!(queue.len(), 1);
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
    event.radius = config.min_radius;
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
