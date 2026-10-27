//! Installed Ares TechnoClass_Update1004B320, through the ordinary AI host.
use super::*;
use crate::rules::terrain_rules::LandType;

#[test]
fn surface_conversion_ai_handles_water_beach_bridge_and_land_in_place() {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=LAND\n1=WATER\n\
         [LAND]\nStrength=100\nSpeed=5\nSpeedType=Hover\nMovementZone=Amphibious\n\
         Convert.Water=WATER\nLocomotor={4A582742-9839-11d1-B709-00A024DDAFD1}\n\
         [WATER]\nStrength=200\nSpeed=5\nSpeedType=Hover\nMovementZone=Amphibious\n\
         Convert.Land=LAND\nConvert.Water=WATER\n\
         AttachEffect.Duration=30\nAttachEffect.InitialDelay=2\nAttachEffect.FirepowerMultiplier=3\n\
         Locomotor={4A582742-9839-11d1-B709-00A024DDAFD1}\n",
    );
    let rules = RuleSet::from_ini(&ini).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 29);
    let id = sim
        .spawn_object("LAND", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let owner = sim.entities().get(id).unwrap().owner();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .power_off();
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(10, 10)
        .unwrap()
        .land_type = LandType::Water.as_index();
    sim.take_lifecycle_outputs_for_test();
    // The new descriptor reaches attached effects in the same AI head.
    for frame in 100..103 {
        sim.session.set_binary_frame_for_test(frame);
        sim.object_ai_stage(Some(&rules));
    }
    let actor = sim.entities().get(id).unwrap();
    assert_eq!(
        sim.interner.resolve(actor.type_ref()),
        if cfg!(feature = "ares") {
            "WATER"
        } else {
            "LAND"
        }
    );
    assert_eq!(
        actor.health.current(),
        if cfg!(feature = "ares") { 200 } else { 100 }
    );
    assert_eq!(actor.owner(), owner);
    assert_eq!(sim.entities().len(), 1);
    assert!(!actor.locomotor.as_ref().unwrap().is_powered());
    #[cfg(feature = "ares")]
    {
        assert_eq!(
            actor.crate_multipliers.firepower(),
            crate::util::native_x87::NativeF64Bits::from_bits(3.0_f64.to_bits())
        );
        assert_eq!(sim.take_lifecycle_outputs_for_test().into_iter().filter(|event|
            matches!(event, crate::sim::world::LifecycleOutput::ObjectTypeChanged { stable_id } if *stable_id == id)
        ).count(), 1);
        // A self pointer must not call ConvertTechno again and overwrite
        // the existing damage reservation with actual health.
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .estimated_health
            .reset(7);
        sim.session.set_binary_frame_for_test(104); // no stock recovery pulse
        sim.object_ai_stage(Some(&rules));
        assert_eq!(sim.entities().get(id).unwrap().estimated_health.get(), 7);
    }
    // OnBridge overrides the water beneath; Beach is water off the bridge.
    // Finish on actual land, exercising the reverse binding once more.
    for (land, bridge, expected) in [
        (LandType::Water, true, "LAND"),
        (LandType::Beach, false, "WATER"),
        (LandType::Clear, false, "LAND"),
    ] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .set_on_bridge(bridge);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(10, 10)
            .unwrap()
            .land_type = land.as_index();
        sim.object_ai_stage(Some(&rules));
        let actor = sim.entities().get(id).unwrap();
        assert_eq!(
            sim.interner.resolve(actor.type_ref()),
            if cfg!(feature = "ares") {
                expected
            } else {
                "LAND"
            }
        );
        assert_eq!(actor.owner(), owner);
        assert!(!actor.locomotor.as_ref().unwrap().is_powered());
        assert_eq!(sim.entities().len(), 1);
    }
}
