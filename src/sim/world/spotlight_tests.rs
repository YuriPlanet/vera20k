use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::{combat::Health, game_entity::GameEntity};

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str("[General]\nSpotlightMovementRadius=2000\nSpotlightLocationRadius=1000\nSpotlightSpeed=.015\nSpotlightAcceleration=.0025\nSpotlightAngle=.5\n[VehicleTypes]\n0=TANK\n[TANK]\nStrength=100\n[BuildingTypes]\n0=LIGHT\n[LIGHT]\nHasSpotlight=yes\nStrength=100\nFoundation=1x1\n")).unwrap()
}

#[test]
fn overflowed_authored_spotlight_motion_keeps_its_last_pose_and_cleanup() {
    for speed in ["1e99", "1e38"] {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nSpotlightSpeed={speed}\n[BuildingTypes]\n0=LIGHT\n\
         [LIGHT]\nHasSpotlight=yes\nStrength=100\nFoundation=1x1\n",
        )))
        .unwrap();
        assert_eq!(
            f64::from_bits(rules.general.spotlight.speed.bits()).is_finite(),
            speed == "1e38"
        );
        let mut sim = Simulation::with_seed(9);
        let p = parent(&mut sim, EntityCategory::Structure);
        sim.construct_spotlight(p, Some(&rules));
        let id = sim
            .substrate
            .entities
            .get(p)
            .unwrap()
            .building_light
            .light()
            .unwrap();
        let before = sim.substrate.spotlights.get(id).unwrap().coord;
        sim.set_spotlight_behavior(id, 2);
        sim.visit_spotlight(id, &rules, UninitContext::new(Some(&rules), None));
        assert_eq!(sim.substrate.spotlights.get(id).unwrap().coord, before);
        assert!(sim.building_spotlight(id, &rules).is_some());
        // Invalid authored motion must not bypass ordinary orphan cleanup.
        sim.spotlights_pointer_expired(p);
        sim.visit_spotlight(id, &rules, UninitContext::new(Some(&rules), None));
        assert!(
            sim.substrate
                .pending_delete
                .object_tickets()
                .any(|i| i == id)
        );
    }
}
fn parent(sim: &mut Simulation, category: EntityCategory) -> u64 {
    parent_id(sim, category, 1)
}
fn parent_id(sim: &mut Simulation, category: EntityCategory, id: u64) -> u64 {
    let owner = sim.interner.intern("Americans");
    let ty = sim
        .interner
        .intern(if category == EntityCategory::Structure {
            "LIGHT"
        } else {
            "TANK"
        });
    let mut e = GameEntity::new_at_frame_zero_for_test(
        id,
        12,
        16,
        0,
        0,
        owner,
        Health::new(100),
        ty,
        category,
        0,
        3,
        false,
    );
    e.position = crate::sim::components::Position::new(
        12,
        16,
        0,
        Some(384),
        crate::util::fixed_math::SimFixed::from_num(128),
        crate::util::fixed_math::SimFixed::ZERO,
    );
    e.building_light.stamp(true);
    e.lifecycle.set_in_limbo_for_test(false);
    sim.substrate.entities.insert(e);
    sim.substrate.next_stable_object_id = sim.substrate.next_stable_object_id.max(id + 1);
    id
}
#[test]
fn original_constructor_endpoints_and_full_ai_motion_controls() {
    let corpus: &serde_json::Value =
        crate::test_fixture::json("tools/spatial_oracle/spotlight.json");
    let params = rules().general.spotlight;
    for case in corpus["constructors"].as_array().unwrap() {
        let p: [i32; 3] = serde_json::from_value(case["parent"].clone()).unwrap();
        let facing = case["facing"].as_u64().unwrap() as u16;
        assert_eq!(
            initial_endpoint(p, facing, params.location_radius),
            serde_json::from_value::<[i32; 3]>(case["endpoint"].clone()).unwrap()
        );
        assert_eq!(
            initial_endpoint(p, facing, -params.movement_radius),
            serde_json::from_value::<[i32; 3]>(case["pivot"].clone()).unwrap()
        );
    }
    let rules = rules();
    for case in corpus["motions"].as_array().unwrap() {
        let mut sim = Simulation::with_seed(19);
        let parent = parent(&mut sim, EntityCategory::Unit);
        let c = &case["constructor"];
        let endpoint = serde_json::from_value(c["endpoint"].clone()).unwrap();
        sim.substrate.spotlights.objects.insert(
            2,
            Spotlight {
                stable_id: 2,
                native_id: 10,
                coord: endpoint,
                pivot: serde_json::from_value(c["pivot"].clone()).unwrap(),
                endpoint,
                angle: NativeF64Bits::from_bits(0),
                rate: NativeF64Bits::from_bits(0),
                increasing: case["direction"].as_bool().unwrap(),
                mode: case["mode"].as_i64().unwrap() as i32,
                target: None,
                parent: Some(parent),
                alive: true,
                in_logic_vector: false,
            },
        );
        for frame in 1..=160 {
            sim.visit_spotlight(2, &rules, UninitContext::new(Some(&rules), None));
            if let Some(expected) = case["states"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["frame"] == frame)
            {
                let light = sim.substrate.spotlights.get(2).unwrap();
                assert_eq!(
                    light.coord,
                    serde_json::from_value::<[i32; 3]>(expected["coord"].clone()).unwrap(),
                    "mode{} frame{frame}",
                    light.mode
                );
                assert_eq!(
                    format!("{:016x}", light.angle.bits()),
                    expected["angle_bits"].as_str().unwrap()
                );
                assert_eq!(
                    format!("{:016x}", light.rate.bits()),
                    expected["rate_bits"].as_str().unwrap()
                );
                assert_eq!(light.increasing, expected["direction"].as_bool().unwrap());
            }
        }
    }
}
#[test]
fn shared_ids_logic_snapshot_and_parent_limbo_retire_the_independent_child() {
    let rules = rules();
    let mut sim = Simulation::with_seed(9);
    let p = parent(&mut sim, EntityCategory::Structure);
    sim.construct_spotlight(p, Some(&rules));
    let id = sim
        .substrate
        .entities
        .get(p)
        .unwrap()
        .building_light
        .light()
        .unwrap();
    assert!(id > p);
    assert_eq!(
        sim.classify_object(id),
        Some(super::super::substrate::ObjectKind::Spotlight)
    );
    assert!(sim.substrate.logic.as_slice().contains(&id));
    sim.debug_assert_logic_membership_consistent();
    sim.set_spotlight_behavior(id, 0x100);
    let state = sim.substrate.spotlights.get(id).unwrap().clone();
    let bytes = GameSnapshot::save(&sim, 0, 0, "searchlight", 0);
    let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
    loaded.restore_after_snapshot_load().unwrap();
    assert_eq!(loaded.substrate.spotlights.get(id), Some(&state));
    loaded.debug_assert_logic_membership_consistent();
    loaded.destroy_spotlight_for_parent(p, UninitContext::new(Some(&rules), None));
    assert!(!loaded.substrate.spotlights.contains_key(id));
    assert!(!loaded.substrate.logic.as_slice().contains(&id));
    assert_eq!(
        loaded
            .substrate
            .entities
            .get(p)
            .unwrap()
            .building_light
            .light(),
        None
    );
}

fn map(event: i32, action: i32, value: i32) -> crate::map::map_file::MapFile {
    crate::map::map_file::MapFile::from_bytes(format!("[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Basic]\nNewINIFormat=4\n[Triggers]\nT=Neutral,<none>,Light,0,1,1,1,0\nU=Neutral,<none>,Other,0,1,1,1,0\n[Events]\nT=1,{event},0,0\nU=1,35,0,0\n[Actions]\nT=1,{action},0,{value},0,0,0,0,A\nU=1,56,0,1,0,0,0,0,A\n[Tags]\nTAG=0,Light,T\nOTHER=0,Other,U\n[VariableNames]\n0=Seen,0\n1=Other,0\n").as_bytes()).unwrap()
}
fn init(sim: &mut Simulation, map: &crate::map::map_file::MapFile) {
    sim.initialize_trigger_definitions(
        map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
    sim.classify_map_triggers(None);
    sim.session.set_started(true);
}
#[test]
fn spotlight_action52_counts_only_matching_live_trigger_buildings_and_keeps_full_scalar() {
    for mode in [0, 256, -7, 3] {
        let rules = rules();
        let mut sim = Simulation::with_seed(8);
        let first = parent(&mut sim, EntityCategory::Structure);
        sim.substrate
            .entities
            .get_mut(first)
            .unwrap()
            .lifecycle
            .set_cell_marked(true);
        sim.construct_spotlight(first, Some(&rules));
        let second = parent_id(&mut sim, EntityCategory::Structure, 3);
        sim.substrate
            .entities
            .get_mut(second)
            .unwrap()
            .lifecycle
            .set_cell_marked(true);
        sim.construct_spotlight(second, Some(&rules));
        let a = sim
            .substrate
            .entities
            .get(first)
            .unwrap()
            .building_light
            .light()
            .unwrap();
        let b = sim
            .substrate
            .entities
            .get(second)
            .unwrap()
            .building_light
            .light()
            .unwrap();
        let map = map(13, 52, mode);
        init(&mut sim, &map);
        sim.attach_object_trigger_tag(first, Some("TAG"));
        sim.attach_object_trigger_tag(second, Some("OTHER"));
        sim.advance_triggers(Some(&rules), None, super::super::FrameEffects::default());
        assert_eq!(sim.substrate.spotlights.get(a).unwrap().mode, mode);
        assert_eq!(sim.substrate.spotlights.get(b).unwrap().mode, 1);
    }
}
#[test]
fn spotlight_event35_uses_child_ground_lists_mode_gate_and_synchronous_tag_action() {
    for (mode, layer, offset, seen) in [
        (1, MovementLayer::Ground, 0, true),
        (1, MovementLayer::Bridge, 0, false),
        (2, MovementLayer::Ground, 0, false),
        (1, MovementLayer::Ground, 300, false),
    ] {
        let rules = rules();
        let mut sim = Simulation::with_seed(8);
        let p = parent(&mut sim, EntityCategory::Structure);
        sim.construct_spotlight(p, Some(&rules));
        let id = sim
            .substrate
            .entities
            .get(p)
            .unwrap()
            .building_light
            .light()
            .unwrap();
        let c = sim.substrate.spotlights.get(id).unwrap().coord;
        let enemy = sim.interner.intern("Enemy");
        let ty = sim.interner.intern("TANK");
        let mut unit = GameEntity::new_at_frame_zero_for_test(
            3,
            0,
            0,
            0,
            0,
            enemy,
            Health::new(0),
            ty,
            EntityCategory::Unit,
            0,
            3,
            false,
        );
        let at = [c[0] + offset, c[1], c[2]];
        unit.position
            .put_location(crate::sim::components::DriveCoord {
                x: at[0],
                y: at[1],
                z: at[2],
            });
        sim.substrate.entities.insert(unit);
        sim.substrate.next_stable_object_id = 4;
        sim.substrate.occupancy.add(
            (at[0] >> 8) as u16,
            (at[1] >> 8) as u16,
            3,
            layer,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
        let map = map(35, 56, 0);
        init(&mut sim, &map);
        sim.attach_object_trigger_tag(p, Some("TAG"));
        sim.set_spotlight_behavior(id, mode);
        sim.object_ai_visit_one(
            id,
            Some(&rules),
            super::super::techno_ai::ObjectAiCtx::default(),
        );
        assert_eq!(
            sim.trigger_local_is_set_for_test(0),
            seen,
            "mode{mode} layer{layer:?} offset{offset}"
        );
        assert_eq!(
            sim.tag_dispatch_trace_for_test()
                .iter()
                .any(|&(_, kind)| kind == 35),
            seen
        );
    }
}
#[test]
fn spotlight_cone_geometry_matches_original_before_projection() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/spotlight.json");
    let rules = rules();
    for row in corpus["draw_geometry"].as_array().unwrap() {
        let mut sim = Simulation::with_seed(8);
        let p = parent(&mut sim, EntityCategory::Structure);
        sim.construct_spotlight(p, Some(&rules));
        let id = sim
            .substrate
            .entities
            .get(p)
            .unwrap()
            .building_light
            .light()
            .unwrap();
        sim.substrate.spotlights.get_mut(id).unwrap().coord =
            serde_json::from_value(row["coord"].clone()).unwrap();
        let light = sim
            .building_spotlight(id, &rules)
            .expect("operational spotlight draws");
        assert_eq!(
            light.beam_origin,
            serde_json::from_value::<[i32; 3]>(row["origin"].clone()).unwrap()
        );
        assert_eq!(
            light.beam_edges,
            Some(serde_json::from_value::<[[i32; 3]; 2]>(row["edges"].clone()).unwrap())
        );
    }
}

#[test]
fn spotlight_real_building_unlimbo_registers_the_child_and_limbo_cleans_both_owners() {
    use crate::sim::superweapon::chronosphere_tests::{place, world_with};
    let (rules, mut sim, _) = world_with(rules(), 64, &[]);
    let p = place(&mut sim, &rules, "LIGHT", (20, 20), 0, false, false);
    let id = sim
        .substrate
        .entities
        .get(p)
        .unwrap()
        .building_light
        .light()
        .expect("successful Building Unlimbo constructs its child");
    assert!(id > p);
    assert!(sim.substrate.spotlights.get(id).unwrap().in_logic_vector);
    let order = sim.substrate.logic.as_slice();
    assert!(
        order.iter().position(|&x| x == id).unwrap() < order.iter().position(|&x| x == p).unwrap()
    );
    sim.uninit_with_context(p, UninitContext::new(Some(&rules), None));
    assert!(!sim.substrate.spotlights.contains_key(id));
    assert_eq!(
        sim.substrate
            .entities
            .get(p)
            .unwrap()
            .building_light
            .light(),
        None
    );
    sim.debug_assert_logic_membership_consistent();
}
#[test]
fn spotlight_tracking_selects_first_nearest_enemy_and_expiry_clears_the_pointer() {
    let rules = rules();
    let mut sim = Simulation::with_seed(8);
    let p = parent(&mut sim, EntityCategory::Structure);
    sim.construct_spotlight(p, Some(&rules));
    let id = sim
        .substrate
        .entities
        .get(p)
        .unwrap()
        .building_light
        .light()
        .unwrap();
    let c = sim.substrate.spotlights.get(id).unwrap().coord;
    for (unit_id, house) in [(3, "Enemy"), (4, "Enemy"), (5, "Americans")] {
        let owner = sim.interner.intern(house);
        let ty = sim.interner.intern("TANK");
        let mut e = GameEntity::new_at_frame_zero_for_test(
            unit_id,
            0,
            0,
            0,
            0,
            owner,
            Health::new(0),
            ty,
            EntityCategory::Unit,
            0,
            3,
            false,
        );
        e.position.put_location(crate::sim::components::DriveCoord {
            x: c[0],
            y: c[1],
            z: c[2],
        });
        sim.substrate.entities.insert(e);
        sim.substrate.occupancy.add(
            (c[0] / 256) as u16,
            (c[1] / 256) as u16,
            unit_id,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
    }
    sim.substrate.next_stable_object_id = 6;
    sim.set_spotlight_behavior(id, 3);
    assert_eq!(sim.substrate.spotlights.get(id).unwrap().target, Some(4));
    sim.set_spotlight_behavior(id, 2);
    assert_eq!(sim.substrate.spotlights.get(id).unwrap().target, Some(4));
    sim.notify_pointer_expired(4, UninitContext::new(Some(&rules), None));
    assert_eq!(sim.substrate.spotlights.get(id).unwrap().target, None);
}
