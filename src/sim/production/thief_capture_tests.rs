use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::fixed_math::SimFixed;

const EXTRA: &str = "\
[InfantryTypes]\n2=THIEF\n3=VEHICLEONLY\n\
[VehicleTypes]\n0=CAR\n\
[THIEF]\nThief=yes\nVehicleThief=yes\nBuildLimit=1\n\
Strength=75\nSpeed=4\nSpeedType=Foot\n\
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
[VEHICLEONLY]\nVehicleThief=yes\nStrength=75\nSpeed=4\nSpeedType=Foot\n\
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
[CAR]\nStrength=300\nSpeed=6\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n";

fn scene() -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
) {
    let (mut sim, rules, overlays) =
        crate::sim::world::entry_test_fixture::fixture_with_rules(EXTRA);
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(owner, side, None, false, 0, 10));
    }
    (sim, rules, overlays)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, x: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, x, 15, 0, 0, rules)
        .unwrap()
}

fn approach(sim: &mut Simulation, rules: &RuleSet, actor: u64, target: u64) {
    sim.substrate
        .mission_assign_exact(
            actor,
            MissionId::from_known(MissionType::Capture),
            sim.session.binary_frame(),
        )
        .unwrap();
    sim.assign_destination_represented(
        actor,
        Some(NavTargetRef::object(target)),
        rules,
        None,
        FrameEffects::default(),
    )
    .unwrap();
}

/// The normal Infantry AI continuation reaches 5202F0 before its second
/// Ready/Commence and Fear, and successful capture consumes that continuation.
#[test]
fn thief_capture_normal_ai_changes_owner_and_retains_captor_type() {
    let (mut sim, rules, overlays) = scene();
    let actor = spawn(&mut sim, &rules, "THIEF", "Americans", 15);
    let target = spawn(&mut sim, &rules, "CAR", "Russians", 16);
    // Keep both in their own marked cells, 70 leptons apart across the edge.
    {
        let actor = sim.substrate.entities.get_mut(actor).unwrap();
        actor.position.set_sub_x_for_test(SimFixed::from_num(250));
        actor.position.set_sub_y_for_test(SimFixed::from_num(128));
    }
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .position
        .set_sub_x_for_test(SimFixed::from_num(64));
    approach(&mut sim, &rules, actor, target);
    let from = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(actor).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    let to = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(target).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    assert_eq!([to.x - from.x, to.y - from.y, to.z - from.z], [70, 0, 0]);
    let rng = sim.scenario_rng.state();

    sim.infantry_action_turn(actor, &rules, None, FrameEffects::default())
        .unwrap();

    let car = sim.substrate.entities.get(target).unwrap();
    assert_eq!(sim.interner.resolve(car.owner()), "Americans");
    assert_eq!(car.health.current(), 300);
    assert_eq!(
        car.capture_infantry_type_index.get(),
        rules
            .type_array_index(crate::rules::object_type::ObjectCategory::Infantry, "THIEF",)
            .unwrap()
    );
    assert!(sim.substrate.pending_delete.contains_object(&actor));
    assert_eq!(sim.scenario_rng.state(), rng);
    let owner = car.owner();
    let thief = sim.interner.get("THIEF").unwrap();
    // UnInit queues the actor; ctor-to-dtor tracking still includes it.
    // Infantry destructor517E2E removes tracking in the normal tail drain.
    assert_eq!(
        sim.houses[&owner]
            .tracking()
            .owned_count(EntityCategory::Infantry, thief),
        1
    );
    sim.process_pending_delete_with(&rules, Some(&overlays), FrameEffects::default(), 1, true);
    assert!(!sim.substrate.entities.contains(actor));
    assert_eq!(
        sim.houses[&owner]
            .tracking()
            .owned_count(EntityCategory::Infantry, thief),
        0
    );
    assert!(super::check_build_limit(
        &sim,
        &rules,
        owner,
        rules.object("THIEF").unwrap(),
    ));
    #[cfg(not(feature = "ares"))]
    assert_eq!(
        super::can_build(
            &sim,
            &rules,
            owner,
            rules.object("THIEF").unwrap(),
            true,
            false,
        ),
        super::CanBuild::AtLimit
    );
    let other = sim.interner.get("Russians").unwrap();
    sim.change_owner_with_rules(target, other, &rules, None, FrameEffects::default());
    assert!(!super::check_build_limit(
        &sim,
        &rules,
        owner,
        rules.object("THIEF").unwrap(),
    ));
    #[cfg(not(feature = "ares"))]
    assert_eq!(
        super::can_build(
            &sim,
            &rules,
            owner,
            rules.object("THIEF").unwrap(),
            true,
            false,
        ),
        super::CanBuild::Yes
    );
}

#[test]
fn thief_capture_keeps_the_original_role_alliance_and_distance_gates() {
    use crate::sim::components::DriveCoord;
    // EC6 alone cannot take a vehicle in an original build. Ares replaces
    // the EC5 gate with its own admission; that is a separate feature.
    let (mut sim, rules, _) = scene();
    let actor = spawn(&mut sim, &rules, "VEHICLEONLY", "Americans", 15);
    let target = spawn(&mut sim, &rules, "CAR", "Russians", 16);
    approach(&mut sim, &rules, actor, target);
    assert!(
        !sim.infantry_try_thief_capture(actor, &rules, None, FrameEffects::default())
            .unwrap()
    );
    assert_eq!(
        sim.interner
            .resolve(sim.substrate.entities.get(target).unwrap().owner()),
        "Russians"
    );

    let (mut sim, rules, _) = scene();
    let actor = spawn(&mut sim, &rules, "THIEF", "Americans", 15);
    let target = spawn(&mut sim, &rules, "CAR", "Americans", 16);
    approach(&mut sim, &rules, actor, target);
    assert!(
        !sim.infantry_try_thief_capture(actor, &rules, None, FrameEffects::default())
            .unwrap()
    );

    let (mut sim, rules, _) = scene();
    let actor = spawn(&mut sim, &rules, "THIEF", "Americans", 15);
    let target = spawn(&mut sim, &rules, "CAR", "Russians", 16);
    {
        let actor = sim.substrate.entities.get_mut(actor).unwrap();
        actor.position.set_sub_x_for_test(SimFixed::from_num(250));
        actor.position.set_sub_y_for_test(SimFixed::from_num(128));
    }
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .position
        .set_sub_x_for_test(SimFixed::from_num(122)); // Exactly 128 away.
    approach(&mut sim, &rules, actor, target);
    let to = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(target).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    let retained = DriveCoord {
        x: to.x + 128,
        y: to.y,
        z: to.z,
    };
    sim.substrate
        .entities
        .get_mut(actor)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .set_walk_destination(Some(retained));
    // A paid step can differ from the retained final request. Using Head_To
    // here would spuriously reissue the destination and disturb the route.
    sim.substrate
        .entities
        .get_mut(actor)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .set_step_head(Some(DriveCoord {
            x: to.x - 256,
            y: to.y,
            z: to.z,
        }));
    let rng = sim.scenario_rng.state();
    assert!(
        !sim.infantry_try_thief_capture(actor, &rules, None, FrameEffects::default())
            .unwrap()
    );
    assert_eq!(
        crate::sim::movement::motion_query::destination(sim.substrate.entities.get(actor).unwrap(),),
        Some(retained),
        "128 away does not capture; a request 128 from the target is retained"
    );
    assert_eq!(sim.scenario_rng.state(), rng);
    sim.substrate.entities.get_mut(actor).unwrap().locomotor = Some(
        crate::sim::movement::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Teleport,
        ),
    );
    let actor_id = actor;
    let actor = sim.substrate.entities.get(actor_id).unwrap();
    assert_eq!(
        crate::sim::movement::motion_query::destination(actor),
        Some(crate::sim::movement::ground_pose::object_get_coords(
            actor,
            sim.resolved_terrain.as_ref()
        ),),
        "a stopped Teleport destination is Location, not NullCoord"
    );
    let actor = sim.substrate.entities.get_mut(actor_id).unwrap();
    actor.install_teleport_state_for_test(Some(
        crate::sim::movement::teleport_movement::TeleportState::for_test(
            crate::sim::movement::teleport_movement::TeleportPhase::Relocate,
            16,
            15,
            0,
        ),
    ));
    actor
        .locomotor
        .as_mut()
        .unwrap()
        .teleport_runtime_mut()
        .unwrap()
        .set_resolved_destination(DriveCoord::cell(20, 15, 0));
    assert_eq!(
        crate::sim::movement::motion_query::destination(actor),
        Some(DriveCoord::cell(16, 15, 0)),
        "moving Teleport reads the armed request, not its reservation"
    );
}
