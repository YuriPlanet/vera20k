//! Locomotor unit tests — verifies locomotor state initialization, speed type mapping,
//! and ObjectType-to-LocomotorState conversion for various unit categories.

use super::*;
use crate::rules::jumpjet_params::JumpjetParams;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::rules::object_type::{ObjectCategory, ObjectType, PipScale};
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO, SimFixed, sim_from_f32};

#[test]
fn walk_destination_and_cell_producer_match_original_startup_conversion() {
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::sim::{components::DriveCoord, game_entity::GameEntity};
    let native: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/walk_head_occupation.json"
    ))
    .unwrap();
    let rows = native["destination"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    for row in rows {
        let input = &row["input"];
        let output = &row["output"];
        let mut terrain = ResolvedTerrainGrid::from_cells(
            11,
            11,
            (0..11)
                .flat_map(|y| {
                    (0..11).map(move |x| {
                        crate::sim::world::common_raw_test_terrain_cell(x, y, 2, false)
                    })
                })
                .collect(),
        );
        let c = terrain.cell_mut(10, 10).unwrap();
        c.slope_type = 1;
        c.bridge_facts.raw_flags = if input["structural"].as_bool().unwrap() {
            0x100
        } else {
            0
        };
        let mut entity = GameEntity::test_default(1, "E1", "Owner", 9, 10);
        entity.locomotor = Some(LocomotorState::from_object_type(
            &make_obj(LocomotorKind::Walk, ObjectCategory::Infantry),
            0,
        ));
        let coord = if input["cell_target"].as_bool().unwrap() {
            crate::sim::movement::navcom::target_cell_coord(10, 10, Some(&terrain))
        } else {
            let c = &input["coord"];
            DriveCoord {
                x: c[0].as_i64().unwrap() as i32,
                y: c[1].as_i64().unwrap() as i32,
                z: c[2].as_i64().unwrap() as i32,
            }
        };
        assert_eq!(
            serde_json::json!([coord.x, coord.y, coord.z]),
            output["incoming"],
            "{row}"
        );
        crate::sim::movement::set_walk_destination_coord(&mut entity, coord, Some(&terrain));
        let loco = entity.locomotor.as_ref().unwrap();
        let dest = loco.walk_destination().unwrap();
        assert_eq!(
            serde_json::json!([dest.x, dest.y, dest.z]),
            output["destination"],
            "{row}"
        );
        assert_eq!(loco.walk_is_moving(), output["moving"].as_bool(), "{row}");
        assert_eq!(
            loco.step_head(),
            None,
            "the destination setter never accepts a head"
        );
    }
}

#[test]
fn walk_moving_byte_matches_original_setter_and_head_lifetime_traces() {
    use crate::sim::components::DriveCoord;
    let native: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/walk_head_occupation.json"
    ))
    .unwrap();
    let cases = native["moving"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    let coord = DriveCoord {
        x: 2752,
        y: 2624,
        z: 0,
    };
    let xyz = |value: Option<DriveCoord>| value.map_or([0, 0, 0], |c| [c.x, c.y, c.z]);
    for case in cases {
        let mut loco = LocomotorState::from_object_type(
            &make_obj(LocomotorKind::Walk, ObjectCategory::Infantry),
            0,
        );
        let actions = case["actions"].as_array().unwrap();
        let trace = case["output"]["trace"].as_array().unwrap();
        assert_eq!(trace.len(), actions.len() + 1);
        for (index, expected) in trace.iter().enumerate() {
            if index > 0 {
                match actions[index - 1].as_str().unwrap() {
                    "move" => loco.set_walk_destination(Some(coord)),
                    "stop" => loco.set_walk_destination(None),
                    // Supplied private-head transitions isolate this byte's
                    // lifetime; production placement/raw has its own corpus.
                    "head" => loco.set_step_head(Some(coord)),
                    "retire" => loco.set_step_head(None),
                    action => panic!("unknown original action {action}"),
                }
            }
            assert_eq!(
                serde_json::json!({
                    "moving": loco.walk_is_moving().unwrap(),
                    "destination": xyz(loco.walk_destination()),
                    "head": xyz(loco.step_head()),
                }),
                *expected,
                "{case} at {index}"
            );
            // A null destination/head with moving=true is a real callback
            // state, so persistence must not infer this byte from either.
            let restored: LocomotorState =
                serde_json::from_str(&serde_json::to_string(&loco).unwrap()).unwrap();
            assert_eq!(restored.walk_is_moving(), loco.walk_is_moving());
        }
    }
}

/// Helper to create a minimal ObjectType with the given locomotor.
fn make_obj(locomotor: LocomotorKind, category: ObjectCategory) -> ObjectType {
    ObjectType {
        id: "TEST".to_string(),
        category,
        name: None,
        ui_name: None,
        cost: 0,
        soylent: 0,
        factory_plant: false,
        cost_bonuses: [crate::util::native_x87::NativeF32Bits::ONE; 5],
        trainable: true,
        explosion_anims: Vec::new(),
        destroy_anims: Vec::new(),
        strength: 100,
        dont_score: false,
        special_threat_value: 0.0,
        threat_posed: 0,
        my_effectiveness_coefficient: None,
        target_effectiveness_coefficient: None,
        target_special_threat_coefficient: None,
        target_strength_coefficient: None,
        target_distance_coefficient: None,
        armor: "none".to_string(),
        speed: 6,
        walk_rate: 1,
        idle_rate: 0,
        weight: SimFixed::lit("2.0"),
        accel_factor: SimFixed::lit("0.03"),
        decel_factor: SimFixed::lit("0.02"),
        accelerates: true,
        passive: false,
        is_train: false,
        slowdown_distance: 512,
        flight_level: -1,
        is_dropship: false,
        pitch_angle: SimFixed::lit("0.34906585"),
        aux_sound1: None,
        aux_sound2: None,
        sight: 5,
        tech_level: -1,
        build_time_multiplier: 1.0,
        build_time_multiplier_x1000: 1000,
        owner: vec![],
        required_houses: vec![],
        forbidden_houses: vec![],
        ai_base_planning_side: -1,
        ai_build_this: false,
        allowed_to_start_in_multiplayer: true,
        prerequisite: vec![],
        prerequisite_override: vec![],
        build_limit: 0,
        requires_stolen_allied_tech: false,
        requires_stolen_soviet_tech: false,
        requires_stolen_third_tech: false,
        primary: None,
        secondary: None,
        elite_primary: None,
        elite_secondary: None,
        fire_up_frame: 0,
        fire_prone_frame: 0,
        secondary_fire_frame: 0,
        secondary_prone_frame: 0,
        image: "TEST".to_string(),
        power: 0,
        extra_power: 0,
        foundation: "1x1".to_string(),
        pixel_selection_bracket_delta: 0,
        build_cat: None,
        adjacent: 6,
        protect_with_wall: false,
        wants_extra_space: false,
        base_normal: true,
        eligibile_for_ally_building: false,
        crewed: false,
        voice_select: None,
        voice_move: None,
        voice_attack: None,
        voice_harvest: None,
        voice_enter: None,
        voice_capture: None,
        prevent_attack_move: false,
        voice_die: Vec::new(),
        die_sounds: Vec::new(),
        damage_sound: None,
        move_sound: None,
        voice_feedback: None,
        voice_special_attack: None,
        crush_sound: None,
        deploy_sound: None,
        undeploy_sound: None,
        leave_transport_sound: None,
        chrono_in_sound: None,
        chrono_out_sound: None,
        has_turret: false,
        turret_rot: 0,
        turret_anim: None,
        turret_anim_is_voxel: false,
        turret_anim_x: 0,
        turret_anim_y: 0,
        turret_anim_z_adjust: 0,
        guard_range: None,
        air_range_bonus: None,
        opportunity_fire: false,
        can_retaliate: true,
        can_passive_acquire: true,
        distributed_fire: false,
        vhp_scan: crate::rules::object_type::VhpScan::None,
        explodes: false,
        veteran_abilities: Default::default(),
        elite_abilities: Default::default(),
        self_healing: false,
        veteran_explodes: false,
        elite_explodes: false,
        veteran_scatter: false,
        elite_scatter: false,
        veteran_cloak: false,
        elite_cloak: false,
        veteran_crusher: false,
        elite_crusher: false,
        death_weapon: None,
        death_weapon_damage_modifier: 1.0,
        super_weapon: None,
        super_weapon2: None,
        spy_sat: false,
        gap_generator: false,
        psychic_detection_radius: 0,
        sensor_array: false,
        sensors: false,
        sensors_sight: 0,
        detect_disguise: false,
        detect_disguise_range: 0,
        cloakable: false,
        cloaking_speed: 1,
        cloak_stop: false,
        cloak_radius_in_cells: 20,
        cloak_generator: false,
        radar: false,
        radar_invisible: false,
        veteran_radar_invisible: false,
        elite_radar_invisible: false,
        radar_visible: false,
        insignificant: false,
        to_protect: false,
        harvester: false,
        spawned: false,
        refinery: false,
        weeder: false,
        dock_unload: false,
        bib: false,
        gate: false,
        deploy_time_ticks: 0,
        gate_close_delay_ticks: 0,
        storage: 0,
        free_unit: None,
        dock: vec![],
        queueing_cell: [0, 0],
        pads: Vec::new(),
        hidden_occupancy: crate::rules::object_type::BuildingHiddenOccupancyProfile::default(),
        base_reservation_spacing: None,
        unloading_class: None,
        ammo: -1,
        initial_ammo: -1,
        spawns: None,
        spawns_number: 0,
        spawn_regen_rate: 0,
        spawn_reload_rate: 0,
        missile_spawn: false,
        no_spawn_alt: false,
        enslaves: None,
        slaves_number: 0,
        slave_regen_rate: 0,
        slave_reload_rate: 0,
        slaved: false,
        fearless: false,
        fraidycat: false,
        crawls: false,
        veteran_fearless: false,
        elite_fearless: false,
        harvest_rate: 0,
        resource_gatherer: false,
        resource_destination: false,
        ore_purifier: false,
        locomotor,
        speed_type: SpeedType::Track,
        movement_zone: MovementZone::Normal,
        movement_restricted_to: None,
        considered_aircraft: false,
        zfudge_cliff: 10,
        zfudge_column: 5,
        zfudge_tunnel: 10,
        zfudge_bridge: 7,
        too_big_to_fit_under_bridge: false,
        crashable: false,
        move_to_shroud: true,
        teleporter: false,
        hover_attack: false,
        balloon_hover: false,
        is_simple_deployer: false,
        deploy_to_land: false,
        airport_bound: false,
        fighter: false,
        fly_by: false,
        fly_back: false,
        landable: false,
        carryall: false,
        jumpjet: false,
        jumpjet_params: JumpjetParams::default(),
        deploys_into: None,
        undeploys_into: None,
        deploy_facing: 0x80,
        construction_yard: false,
        build_const_eligible: false,
        base_plan_type_index: -1,
        is_base_defense: false,
        factory: None,
        weapons_factory: false,
        cloning: false,
        exit_coord: None,
        crushable: false,
        deployed_crushable: true,
        crusher: false,
        no_force_shield: false,
        omni_crusher: false,
        omni_crush_resistant: false,
        immune_to_radiation: false,
        damage_self: false,
        immune: false,
        type_immune: false,
        immune_to_psionics: false,
        warpable: true,
        bombable: true,
        bomb_sight: 0,
        mind_control_ring_offset: 0x8C,
        mind_cleared_sound: None,
        immune_to_psionic_weapons: false,
        immune_to_poison: false,
        engineer: false,
        ivan: false,
        deployer: false,
        capturable: false,
        needs_engineer: false,
        capture_eva_event: None,
        repairable: true,
        can_be_occupied: false,
        can_occupy_fire: false,
        show_occupant_pips: false,
        place_anywhere: false,
        to_tile: None,
        bridge_repair_hut: false,
        laser_fence: false,
        firestorm_wall: false,
        passengers: 0,
        size_limit: 0,
        size: 3,
        open_topped: false,
        gunner: false,
        ifv_mode: 0,
        open_transport_weapon: -1,
        deploy_fire: false,
        deploy_fire_weapon: None,
        max_number_occupants: 0,
        occupier: false,
        assaulter: false,
        occupy_weapon: None,
        elite_occupy_weapon: None,
        occupy_pip: 7,
        pip_scale: PipScale::None,
        infantry_absorb: false,
        unit_absorb: false,
        grinding: false,
        bunkerable: category == ObjectCategory::Vehicle,
        weapon_list: vec![None; crate::rules::object_type::WEAPON_SLOT_COUNT],
        elite_weapon_list: vec![None; crate::rules::object_type::WEAPON_SLOT_COUNT],
        weapon_count: 0,
        naval_targeting: 0,
        land_targeting: 0,
        underwater: false,
        organic: false,
        parasiteable: true,
        suppression_threshold: 0,
        reselect_if_limboed: false,
        rejoin_team_if_limboed: false,
        unnatural: false,
        natural: false,
        pushy: false,
        berserk_friendly: false,
        mobile_fire: true,
        hunter_seeker: false,
        non_vehicle: false,
        jumpjet_turn: false,
        emp_pulse_cannon: false,
        is_gattling: false,
        artillary: false,
        gattling_stages: Default::default(),
        turret_count: 0,
        drainable: false,
        produce_cash_startup: 0,
        produce_cash_amount: 0,
        produce_cash_delay: 0,
        overpowerable: false,
        attack_cursor_on_friendlies: false,
        sabotage_cursor: false,
        c4: false,
        can_c4: false,
        eligible_for_delay_kill: false,
        invisible: false,
        invisible_in_game: false,
        unit_repair: false,
        bunker: false,
        unit_reload: false,
        helipad: false,
        number_of_docks: 1,
        toggle_power: false,
        powered: false,
        powered_special: false,
        can_disguise: false,
        disguise_when_still: false,
        wall: false,
        to_overlay: None,
        unsellable: false,
        click_repairable: true,
        selectable: true,
        light_visibility: 0,
        light_intensity: 0.0,
        has_spotlight: false,
        light_red_tint: 1.0,
        light_green_tint: 1.0,
        light_blue_tint: 1.0,
        water_bound: false,
        naval: false,
        number_impassable_rows: -1,
        natural_particle_system: None,
        natural_particle_location: glam::IVec3::ZERO,
        refinery_smoke_particle_system: None,
        damage_particle_systems: Vec::new(),
        max_debris: 0,
        min_debris: 0,
        debris_types: Vec::new(),
        debris_maximums: Vec::new(),
        debris_anims: Vec::new(),
        close_range: false,
        cyborg: false,
        destroy_particle_systems: Vec::new(),
        damage_smoke_offset: glam::IVec3::ZERO,
        dam_smk_off_scrn_rel: false,
        destroy_smoke_offset: glam::IVec3::ZERO,
        refinery_smoke_offsets: [glam::IVec3::ZERO; 4],
        refinery_smoke_frames: 0,
        gap_radius_in_cells: 0,
        super_gap_radius_in_cells: 0,
        stupid_hunt: false,
        vehicle_thief: false,
        undeploy_delay: -1,
    }
}

#[test]
fn test_drive_locomotor() {
    let obj = make_obj(LocomotorKind::Drive, ObjectCategory::Vehicle);
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.kind, LocomotorKind::Drive);
    assert_eq!(state.layer, MovementLayer::Ground);
    assert_eq!(state.phase, GroundMovePhase::Idle);
    assert_eq!(state.air_phase(), AirMovePhase::Landed);
    assert_eq!(state.speed_multiplier, SIM_ONE);
    assert!(state.is_ground_mover());
    assert!(!state.is_air_mover());
}

#[test]
fn test_hover_cruises_at_full_base_speed() {
    // Hover now cruises at its full base Speed (throttle 1.0), not the old
    // made-up 0.65x. The accel/brake throttle ramp lives in sim/movement/hover.rs.
    let obj = make_obj(LocomotorKind::Hover, ObjectCategory::Vehicle);
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.kind, LocomotorKind::Hover);
    assert_eq!(state.speed_multiplier, SIM_ONE);
    assert!(state.is_ground_mover());
}

#[test]
fn test_walk_locomotor() {
    let obj = make_obj(LocomotorKind::Walk, ObjectCategory::Infantry);
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.kind, LocomotorKind::Walk);
    assert_eq!(state.layer, MovementLayer::Ground);
    assert!(state.is_ground_mover());
}

#[test]
fn test_fly_locomotor_air_layer() {
    let obj = make_obj(LocomotorKind::Fly, ObjectCategory::Aircraft);
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.kind, LocomotorKind::Fly);
    assert_eq!(state.layer, MovementLayer::Air);
    assert_eq!(state.air_phase(), AirMovePhase::Landed);
    assert!(!state.is_ground_mover());
    assert!(state.is_air_mover());
    assert_eq!(state.fly_target_height(), 0);
    assert_eq!(
        state.speed_fraction, SIM_ZERO,
        "constructor4CC9E5 target speed"
    );
}

#[test]
fn fly_target_uses_type_flight_level_without_changing_other_locomotors() {
    let mut obj = make_obj(LocomotorKind::Fly, ObjectCategory::Aircraft);
    for (configured, expected) in [(-1, 1500), (0, 0), (-2, -2), (2200, 2200)] {
        obj.flight_level = configured;
        let mut state = LocomotorState::from_object_type(&obj, 0);
        assert_eq!(state.fly_target_height(), 0, "constructor4CC9EE");
        state.begin_fly_takeoff(obj.flight_level(1500));
        assert_eq!(
            state.fly_target_height(),
            expected,
            "admitted BeginTakeoff4CF9F8"
        );
    }
    obj.flight_level = 2200;
    obj.locomotor = LocomotorKind::Jumpjet;
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(
        state.jumpjet_runtime().unwrap().params.height,
        obj.jumpjet_params.height
    );
    assert!(state.fly_runtime().is_none());
}

#[test]
fn test_jumpjet_air_layer() {
    let obj = make_obj(LocomotorKind::Jumpjet, ObjectCategory::Infantry);
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.kind, LocomotorKind::Jumpjet);
    assert_eq!(state.layer, MovementLayer::Air);
    assert!(!state.is_ground_mover());
    assert!(state.is_air_mover());
    assert_eq!(state.jumpjet_runtime().unwrap().params.height, 500);
}

#[test]
fn test_jumpjet_with_custom_params() {
    let mut obj = make_obj(LocomotorKind::Jumpjet, ObjectCategory::Infantry);
    obj.jumpjet = true;
    obj.jumpjet_params = JumpjetParams {
        turn_rate: 4,
        speed: sim_from_f32(20.0),
        climb: 8.0,
        crash: 5.0,
        height: 750,
        accel: 2.0,
        wobbles: 0.2,
        deviation: 40,
        no_wobbles: false,
    };
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.jumpjet_runtime().unwrap().params.height, 750);
    assert_eq!(state.jumpjet_speed, sim_from_f32(20.0));
    assert_eq!(
        state.jumpjet_runtime().unwrap().params.climb_bits,
        8.0f32.to_bits()
    );
}

#[test]
fn test_ship_is_ground_mover() {
    let obj = make_obj(LocomotorKind::Ship, ObjectCategory::Vehicle);
    let state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.kind, LocomotorKind::Ship);
    assert!(state.is_ground_mover());
    assert!(!state.is_air_mover());
}

#[test]
fn cmin_locomotor_initializes_primary_and_active_teleport() {
    let mut obj = make_obj(LocomotorKind::Teleport, ObjectCategory::Vehicle);
    obj.harvester = true;
    obj.teleporter = true;
    obj.turret_rot = 5;

    let state = LocomotorState::from_object_type(&obj, 0);

    assert_eq!(state.active_kind(), LocomotorKind::Teleport);
    assert_eq!(state.effective_kind(), LocomotorKind::Teleport);
    assert!(state.is_primary_active());
    assert_eq!(state.rot, 5);
}

#[test]
fn test_is_airborne() {
    let obj = make_obj(LocomotorKind::Fly, ObjectCategory::Aircraft);
    let mut state = LocomotorState::from_object_type(&obj, 0);
    assert!(!state.is_airborne());
    state.altitude = SimFixed::from_num(100);
    assert!(state.is_airborne());
}

// --- Override/Piggyback mechanism tests ---

#[test]
fn test_override_teleport_round_trip() {
    let obj = make_obj(LocomotorKind::Drive, ObjectCategory::Vehicle);
    let mut state = LocomotorState::from_object_type(&obj, 0);
    assert!(!state.is_overridden());
    assert_eq!(state.kind, LocomotorKind::Drive);
    assert_eq!(state.layer, MovementLayer::Ground);

    // Begin teleport override.
    state.begin_piggyback(LocomotorKind::Teleport, MovementLayer::Ground, 0);
    assert!(state.is_overridden());
    assert_eq!(state.kind, LocomotorKind::Teleport);
    assert_eq!(state.layer, MovementLayer::Ground);

    // End override — should restore Drive.
    assert!(state.end_piggyback());
    assert!(!state.is_overridden());
    assert_eq!(state.kind, LocomotorKind::Drive);
    assert_eq!(state.layer, MovementLayer::Ground);
    assert_eq!(state.speed_multiplier, SIM_ONE);
}

#[test]
fn end_piggyback_without_a_stash_reports_nothing_to_pop() {
    let obj = make_obj(LocomotorKind::Drive, ObjectCategory::Vehicle);
    let mut state = LocomotorState::from_object_type(&obj, 0);
    let result = state.end_piggyback();
    assert!(
        !result,
        "ending with nothing stashed reports nothing to pop"
    );
    assert_eq!(state.kind, LocomotorKind::Drive);
}

#[test]
fn test_override_preserves_speed_type() {
    let mut obj = make_obj(LocomotorKind::Drive, ObjectCategory::Vehicle);
    obj.speed_type = SpeedType::Wheel;
    let mut state = LocomotorState::from_object_type(&obj, 0);
    assert_eq!(state.speed_type, SpeedType::Wheel);

    state.begin_piggyback(LocomotorKind::Teleport, MovementLayer::Ground, 0);
    // SpeedType should still reflect the original during override.
    state.end_piggyback();
    assert_eq!(state.speed_type, SpeedType::Wheel);
}

#[test]
fn drive_piggyback_restores_primary_teleport_only_after_not_moving() {
    let obj = make_obj(LocomotorKind::Teleport, ObjectCategory::Vehicle);
    let mut state = LocomotorState::from_object_type(&obj, 0);

    assert!(state.begin_drive_piggyback_for_teleporter(0));
    assert_eq!(state.active_kind(), LocomotorKind::Drive);
    assert_eq!(state.effective_kind(), LocomotorKind::Teleport);
    assert!(!state.can_restore_primary_from_piggyback(true, false, false));
    assert!(!state.can_restore_primary_from_piggyback(false, true, false));
    assert!(!state.can_restore_primary_from_piggyback(false, false, true));
    assert!(state.can_restore_primary_from_piggyback(false, false, false));

    assert!(state.restore_primary_from_piggyback());
    assert_eq!(state.active_kind(), LocomotorKind::Teleport);
    assert_eq!(state.effective_kind(), LocomotorKind::Teleport);
    assert!(state.is_primary_active());
}

#[test]
fn drive_piggyback_refuses_an_unstashed_active_drive() {
    let obj = make_obj(LocomotorKind::Teleport, ObjectCategory::Vehicle);
    let mut state = LocomotorState::from_object_type(&obj, 0);
    state.kind = LocomotorKind::Drive;

    assert!(!state.begin_drive_piggyback_for_teleporter(0));
    assert_eq!(state.kind, LocomotorKind::Drive);
    assert!(state.piggyback.is_none());
}

/// End of the production chain for the two units the `JumpJet=` gate broke:
/// retail INI bytes -> `ObjectType::from_ini_section` -> `LocomotorState`.
///
/// gamemd copies the type's jumpjet block into the locomotor unconditionally
/// once the Jumpjet locomotor is installed (parameter copy at `0x0054AD30`,
/// reading `TechnoType+0xD70`..`+0xD8C`), and `TechnoTypeClass::ReadINI`
/// `0x00715020`-`0x0071520F` filled that block with no reference to
/// `JumpJet=`. So a stock Kirov hovers at its authored 750, not at the
/// constructor's 500.
#[test]
fn retail_kirov_and_disc_reach_their_authored_hover_altitude() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };

    // (section, JumpjetSpeed, JumpjetClimb, JumpjetCrash) as authored.
    for (id, speed, climb, crash) in [("ZEP", 5.0, 6.0_f32, 12.0), ("DISK", 16.0, 8.0, 15.0)] {
        let obj = ObjectType::from_ini_section(
            id,
            ini.section(id).unwrap_or_else(|| panic!("[{id}] section")),
            ObjectCategory::Vehicle,
        );
        let state = LocomotorState::from_object_type(&obj, 0);

        assert_eq!(state.kind, LocomotorKind::Jumpjet, "[{id}]");
        assert_eq!(
            state.jumpjet_runtime().unwrap().params.height,
            750,
            "[{id}] hovers at its authored JumpjetHeight"
        );
        assert_eq!(state.jumpjet_speed, sim_from_f32(speed), "[{id}]");
        assert_eq!(
            state.jumpjet_runtime().unwrap().params.climb_bits,
            climb.to_bits(),
            "[{id}]"
        );
        assert_eq!(
            state.jumpjet_crash_speed,
            (sim_from_f32(climb) + sim_from_f32(crash)) * SimFixed::from_num(15),
            "[{id}] crash descent is (climb + crash)"
        );
        // The constructor's acceleration, because stock spells the key
        // `JumpJetAccel=` and gamemd looks up `JumpjetAccel`.
        assert_eq!(state.jumpjet_accel, sim_from_f32(2.0), "[{id}]");
        assert_eq!(state.jumpjet_turn_rate, 4, "[{id}]");
    }
}

/// Non-Jumpjet locomotors must not pick up the block. Every `TechnoType`
/// carries it, but only the copy at `0x0054AD30` reads it, and that lives in
/// the Jumpjet locomotor.
#[test]
fn non_jumpjet_locomotors_ignore_the_types_jumpjet_block() {
    for kind in [
        LocomotorKind::Drive,
        LocomotorKind::Walk,
        LocomotorKind::Hover,
        LocomotorKind::Fly,
        LocomotorKind::Ship,
    ] {
        let mut obj = make_obj(kind, ObjectCategory::Vehicle);
        // A section that authored every jumpjet key would still not reach a
        // Drive or Fly locomotor.
        obj.jumpjet_params = JumpjetParams {
            turn_rate: 100,
            speed: sim_from_f32(99.0),
            climb: 9.0,
            crash: 9.0,
            height: 750,
            accel: 10.0,
            wobbles: 0.5,
            deviation: 15,
            no_wobbles: true,
        };
        let state = LocomotorState::from_object_type(&obj, 0);

        assert_eq!(
            state.jumpjet_accel,
            crate::util::fixed_math::SIM_ZERO,
            "{kind:?}"
        );
        assert_eq!(state.jumpjet_deviation, 0, "{kind:?}");
        assert_eq!(
            state.jumpjet_crash_speed,
            crate::util::fixed_math::SIM_ZERO,
            "{kind:?}"
        );
        assert_eq!(state.jumpjet_turn_rate, 4, "{kind:?}");
        if kind != LocomotorKind::Fly {
            assert_eq!(state.fly_target_height(), 0, "{kind:?}");
        }
    }
}
