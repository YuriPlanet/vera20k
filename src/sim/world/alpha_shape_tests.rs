use super::*;
use crate::rules::alpha_image_assets::AlphaImage;
use crate::rules::ini_parser::IniFile;
use crate::sim::world::FrameEffects;

/// A map-placed lamp's Unlimbo centres a glow on GetCoords
/// (`0x005F5045..0x005F5100`); a type whose AlphaImage did not load makes
/// none. The lamp's expiry disables the glow (`0x00420E70`), which stays in
/// the vector, still drawn, until the tick's PurgeDisabled (`0x0055B650`).
#[test]
fn a_loaded_alpha_image_glows_from_unlimbo_until_the_purge_after_expiry() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=LAMP\n1=POST\n\
         [LAMP]\nAlphaImage=GLOW\nStrength=100\nFoundation=1x1\n\
         [POST]\nAlphaImage=MISSING\nStrength=100\nFoundation=1x1\n",
    ))
    .unwrap();
    rules.supply_alpha_image_for_test(
        "GLOW",
        AlphaImage::for_test([61, 31], [0, 0, 1, 1], vec![254]),
    );
    let entities = crate::map::entities::parse_map_entities(
        &IniFile::from_str(
            "[Structures]\n\
             0=Americans,LAMP,256,4,4,0,None,0,0,1,0,0,None,None,None,0,0\n\
             1=Americans,POST,256,7,4,0,None,0,0,1,0,0,None,None,None,0,0\n",
        ),
        4,
    );
    let mut sim = Simulation::with_seed(5);
    sim.session.set_map_width_for_test(10);
    sim.session.set_map_height_for_test(10);
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 10,
        off_fc: 0,
        off_100: 0,
        off_104: 10,
        off_108: 10,
    });
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    assert_eq!(sim.spawn_from_map(&entities, Some(&rules)), 2);
    // GetCoords (1152,1152,0) projects to (0,135), plus VERA's 15-pixel row
    // bias, less half the 61x31 canvas.
    assert_eq!(
        sim.alpha_shapes().collect::<Vec<_>>(),
        [([-30, 135], "GLOW")]
    );

    let lamp = sim
        .substrate
        .entities
        .values()
        .find(|entity| entity.position.rx() == 4)
        .unwrap()
        .stable_id();
    sim.detach_all_pointer_expired(lamp, &rules, &mut FrameEffects::default());
    assert_eq!(sim.alpha_shapes().count(), 1);
    sim.advance_tick(&[], Some(&rules), None, None, 33);
    assert_eq!(sim.alpha_shapes().count(), 0);
}

/// Ares 3.0 `UpdateAlphaShape` (hook `0x005F3E70`) re-places a glow from its
/// object's live GetCoords, so a Super Thor's ring travels with the gunship.
/// Stock keeps the Unlimbo spot.
#[test]
fn ares_moves_a_glow_with_its_object() {
    for ares in [false, true] {
        let _mode = crate::rules::ares_switch::read_ares_way(ares);
        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[VehicleTypes]\n0=ODIN\n[ODIN]\nAlphaImage=ODINSH\nStrength=100\nSpeed=6\n\
             [ParticleSystems]\n0=GasCloudSys\n[GasCloudSys]\nBehavesLike=Gas\n\
             [Particles]\n0=Lamp\n[Lamp]\nBehavesLike=Smoke\nMaxEC=100\nAlphaImage=GLOW\n",
        );
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        assert_eq!(rules.ares(), ares);
        rules.supply_alpha_image_for_test(
            "ODINSH",
            AlphaImage::from_frames_for_test(
                [61, 31],
                &[([0, 0, 1, 1], &[254]), ([0, 0, 1, 1], &[64])],
            ),
        );
        rules.supply_alpha_image_for_test(
            "GLOW",
            AlphaImage::for_test([1, 1], [0, 0, 1, 1], vec![200]),
        );
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 5);
        let entities = crate::map::entities::parse_map_entities(
            &IniFile::from_str("[Units]\n0=Americans,ODIN,256,14,14,0,Guard,None,None,-1,0\n"),
            4,
        );
        assert_eq!(sim.spawn_from_map(&entities, Some(&rules)), 1);
        let unit = sim.substrate.entities.values().next().unwrap().stable_id();
        let origin = |sim: &Simulation| {
            sim.alpha_shapes()
                .find(|(_, name)| *name == "ODINSH")
                .unwrap()
                .0
        };
        let born = origin(&sim);
        sim.spawn_gas_particle(
            rules.p_type_id_by_name("Lamp").unwrap(),
            glam::IVec3::new(3712, 3712, 0),
            &rules,
        )
        .unwrap();
        let moved = crate::sim::components::DriveCoord {
            x: 14 * 256 + 224,
            y: 14 * 256 + 128,
            z: 0,
        };
        crate::sim::movement::ground_pose::foot_set_location(
            &mut sim.substrate.entities,
            unit,
            moved,
            &rules,
            &sim.interner,
        );
        assert_eq!(sim.session.binary_frame(), 0);
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        assert_eq!(origin(&sim), born, "frame 0 holds the Unlimbo position");
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(moved.x, moved.y, 0);
        let odd_origin = [x as i32 - 31, y as i32 - 16];
        assert_ne!(odd_origin, born);
        assert_eq!(origin(&sim), if ares { odd_origin } else { born });
        // Rebuilding the vehicle puts it above the stationary particle.
        let names = sim.alpha_shapes().map(|(_, name)| name).collect::<Vec<_>>();
        assert_eq!(
            names,
            if ares {
                ["GLOW", "ODINSH"]
            } else {
                ["ODINSH", "GLOW"]
            }
        );
        let frame = sim.session.binary_frame();
        sim.substrate
            .entities
            .get_mut(unit)
            .unwrap()
            .body_facing
            .snap(0x8000, frame);
        let (_, _, drawn) = sim
            .alpha_shape_draw_frames(&rules)
            .find(|(_, image, _)| image.canvas() == [61, 31])
            .unwrap();
        assert_eq!(
            drawn,
            usize::from(ares),
            "only Ares selects a hull-facing frame"
        );
    }
}

/// The lighthouse's animation spawns a Smoke particle in GasCloudSys. Each
/// successor also enters ObjectClass::Unlimbo through Particle ctor 62BB0E.
#[test]
fn successive_lighthouse_particles_replace_and_retire_their_glows() {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[ParticleSystems]\n0=GasCloudSys\n[GasCloudSys]\nBehavesLike=Gas\n\
         [Particles]\n0=Light01\n1=Light02\n\
         [Light01]\nBehavesLike=Smoke\nMaxEC=1\nAlphaImage=BEAM1\nNextParticle=Light02\n\
         [Light02]\nBehavesLike=Smoke\nMaxEC=1\nAlphaImage=BEAM2\n",
    );
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    for name in ["BEAM1", "BEAM2"] {
        rules.supply_alpha_image_for_test(
            name,
            AlphaImage::for_test([3, 3], [0, 0, 1, 1], vec![180]),
        );
    }
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 5);
    let light = rules.p_type_id_by_name("Light01").unwrap();
    sim.spawn_gas_particle(light, glam::IVec3::new(3200, 3200, 0), &rules)
        .unwrap();
    let names = |sim: &Simulation| {
        sim.alpha_shapes()
            .map(|(_, name)| name.to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&sim), ["BEAM1"]);
    sim.advance_tick(&[], Some(&rules), None, Some(rules.overlay_registry()), 22);
    assert_eq!(names(&sim), ["BEAM2"]);
    sim.advance_tick(&[], Some(&rules), None, Some(rules.overlay_registry()), 22);
    assert!(names(&sim).is_empty());

    // Deleting a whole system also expires a particle that still has a light.
    sim.spawn_gas_particle(light, glam::IVec3::new(3200, 3200, 0), &rules)
        .unwrap();
    let system = sim.particle_systems().gas_cloud_system().unwrap();
    assert_eq!(names(&sim), ["BEAM1"]);
    sim.delete_particle_system_now(system, crate::sim::world::UninitContext::new(&rules));
    sim.advance_tick(&[], Some(&rules), None, Some(rules.overlay_registry()), 22);
    assert!(names(&sim).is_empty());
}
