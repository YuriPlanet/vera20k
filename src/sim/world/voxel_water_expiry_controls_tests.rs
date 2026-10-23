//! Numerical expiry-query controls, separate from the ordinary stock flight.
//!
//! These post-constructor duration/position priors are not natural flights.
//! Original expiry reads retained Object.Location.Z. These controls supply
//! both that query location and the independent Bounce position; the natural
//! flight test supplies neither after construction.

use crate::sim::bounce::BounceState;
use crate::sim::runtime::SimRuntime;
use crate::util::native_x87::NativeF32Bits;

fn loaded() -> Option<crate::headless_scenario::HeadlessScenario> {
    let (root, _) = crate::rules::retail_ini_fixture::retail_assets()?;
    Some(crate::headless_scenario::load(&root, "Tsunami.mmx", 17).unwrap())
}

fn piece_with_prior(runtime: &mut SimRuntime, z: i32, duration: i32) -> u64 {
    let (sim, resources) = (&mut runtime.simulation, &runtime.resources);
    let rules = &resources.rules;
    let kind = rules.voxel_anim_type_id_by_name("TIRE").unwrap();
    sim.emit_debris_voxel(
        kind,
        rules.voxel_anim_type(kind),
        None,
        glam::IVec3::new(19972, 28836, 0),
        true,
        &mut Vec::new(),
    )
    .unwrap();
    let ids = sim.voxel_anims().ids();
    assert_eq!(ids.len(), 1);
    let id = ids[0];
    let piece = sim.substrate.voxel_anims.get_mut(id).unwrap();
    let body = piece.bounce();
    let mut position = body.position();
    position[2] = NativeF32Bits::from_bits((z as f32).to_bits());
    piece.set_bounce_for_test(BounceState::from_native_for_test(
        body.elasticity(),
        body.gravity(),
        body.angular_velocity_magnitude(),
        position,
        body.velocity(),
        body.orientation(),
        body.rotation(),
    ));
    piece.set_duration_for_test(duration);
    piece.set_location_for_test(glam::IVec3::new(19972, 28836, z));
    id
}

#[test]
fn tire_water_expiry_requires_strictly_below_the_deck_plane() {
    let Some(loaded) = loaded() else {
        return;
    };
    let terrain = loaded.runtime.simulation.resolved_terrain().unwrap();
    let cell = terrain.cell(78, 112).unwrap();
    assert_eq!(
        (
            cell.source_tile_index,
            cell.source_sub_tile,
            cell.level,
            cell.slope_type,
            cell.land_type
        ),
        (129, 19, 0, 0, 2)
    );
    let floor =
        crate::util::lepton::ground_height_leptons(cell.level, cell.slope_type, 19972, 28836)
            .unwrap();
    assert_eq!(floor, 0);
    for (height, creates_pair) in [(415, true), (416, false), (417, false)] {
        let mut runtime = super::shoreline_runtime(&loaded);
        let id = piece_with_prior(&mut runtime, floor + height, 1);
        let (sim, resources) = (&mut runtime.simulation, &runtime.resources);
        let rng_before = sim.scenario_rng.native_state_hex();
        sim.visit_voxel_anim(id, &resources.rules, super::FrameEffects::default());
        let piece = sim
            .voxel_anims()
            .get(id)
            .expect("UnInit retains body until late drain");
        assert!(!piece.lifecycle().object_alive());
        assert!(piece.lifecycle().in_limbo());
        assert!(!sim.live_object_order_snapshot().contains(&id));
        assert_eq!(sim.display_layers().layer_of(id), None);
        sim.process_pending_delete_with(
            &resources.rules,
            Some(&resources.overlay_registry),
            super::FrameEffects::default(),
            1,
            sim.session.started(),
        );
        assert!(sim.voxel_anims().get(id).is_none(), "height{height}");
        let children: Vec<_> = sim
            .anims()
            .map(|(_, anim)| {
                (
                    sim.interner.resolve(anim.type_id()).to_owned(),
                    anim.world_coord(),
                )
            })
            .collect();
        if creates_pair {
            assert_eq!(children.len(), 2, "height{height}");
            assert_eq!(children[0].0, "WAKE1");
            assert_eq!(children[1].0, "H2O_EXP3");
            assert_eq!(children[0].1.z, floor + height);
            assert_eq!(children[1].1.z, floor + height + 10);
            for (_, coord) in children {
                assert_eq!((coord.x, coord.y), (19972, 28836));
            }
        } else {
            assert!(children.is_empty(), "height{height}");
        }
        // Physical WAKE/H2O have no nontrivial constructor Scenario draw.
        // Sound-device/Main behavior is outside this direct visitor control.
        assert_eq!(sim.scenario_rng.native_state_hex(), rng_before);
    }
}

#[test]
fn tire_out_of_domain_update_deletes_without_water_expiry_effects() {
    let Some(loaded) = loaded() else {
        return;
    };
    let mut runtime = super::shoreline_runtime(&loaded);
    let id = piece_with_prior(&mut runtime, 0, 150);
    let (sim, resources) = (&mut runtime.simulation, &runtime.resources);
    let piece = sim.substrate.voxel_anims.get_mut(id).unwrap();
    let body = piece.bounce();
    let mut velocity = body.velocity();
    velocity[0] = NativeF32Bits::from_bits(f32::NAN.to_bits());
    piece.set_bounce_for_test(BounceState::from_native_for_test(
        body.elasticity(),
        body.gravity(),
        body.angular_velocity_magnitude(),
        body.position(),
        velocity,
        body.orientation(),
        body.rotation(),
    ));
    let rng_before = sim.scenario_rng.native_state_hex();
    let native_id_before = sim.native_identity_cursor();
    let stable_id_before = sim.next_stable_object_id();
    sim.visit_voxel_anim(id, &resources.rules, super::FrameEffects::default());
    assert!(
        !sim.voxel_anims()
            .get(id)
            .unwrap()
            .lifecycle()
            .object_alive()
    );
    assert!(!sim.live_object_order_snapshot().contains(&id));
    assert_eq!(sim.display_layers().layer_of(id), None);
    sim.process_pending_delete_with(
        &resources.rules,
        Some(&resources.overlay_registry),
        super::FrameEffects::default(),
        1,
        sim.session.started(),
    );
    assert!(sim.voxel_anims().get(id).is_none());
    assert!(sim.anims().next().is_none());
    assert_eq!(sim.native_identity_cursor(), native_id_before);
    assert_eq!(sim.next_stable_object_id(), stable_id_before);
    assert_eq!(sim.scenario_rng.native_state_hex(), rng_before);
    // This exercises VERA's finite-domain safety policy, not native NaN parity.
}
