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
    *sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
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
    assert_eq!(glows(&sim, &rules), [([-30, 135], "GLOW")]);

    let lamp = sim
        .substrate
        .entities
        .values()
        .find(|entity| entity.position.rx() == 4)
        .unwrap()
        .stable_id();
    sim.detach_all_pointer_expired(lamp, &rules, &mut FrameEffects::default());
    assert_eq!(glows(&sim, &rules).len(), 1);
    sim.advance_tick(&[], Some(&rules), None, None, 33);
    assert!(glows(&sim, &rules).is_empty());
}

/// Each glow's origin and AlphaImage, as the A plane draws them.
fn glows<'a>(sim: &'a Simulation, rules: &'a RuleSet) -> Vec<([i32; 2], &'a str)> {
    sim.alpha_shape_draws(rules)
        .map(|draw| (draw.origin, draw.image))
        .collect()
}

/// Ares 3.0 `UpdateAlphaShape` (hook `0x005F3E70`) re-places a glow from its
/// object's live GetCoords on an odd frame, so a Super Thor's ring travels
/// with the gunship. Stock keeps the Unlimbo spot.
#[test]
fn ares_moves_a_glow_with_its_object() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=ODIN\n\
         [ODIN]\nAlphaImage=ODINSH\nStrength=100\nSpeed=6\nImmuneToAbduction=yes\n",
    ))
    .unwrap();
    assert!(rules.ares());
    rules.supply_alpha_image_for_test(
        "ODINSH",
        AlphaImage::for_test([60, 30], [0, 0, 1, 1], vec![254]),
    );
    let entities = crate::map::entities::parse_map_entities(
        &IniFile::from_str("[Units]\n0=Americans,ODIN,256,4,4,0,None,None,None,-1,0\n"),
        4,
    );
    let mut sim = Simulation::with_seed(5);
    sim.session.set_map_width_for_test(10);
    sim.session.set_map_height_for_test(10);
    *sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
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
    assert_eq!(sim.spawn_from_map(&entities, Some(&rules)), 1);
    let unit = sim.substrate.entities.values().next().unwrap().stable_id();
    let born = glows(&sim, &rules)[0].0;

    // Two cells east: the glow takes the same screen delta, not the Unlimbo
    // spot the stock implementation keeps.
    let moved = crate::sim::components::DriveCoord {
        x: 6 * 256 + 128,
        y: 4 * 256 + 128,
        z: 0,
    };
    crate::sim::movement::ground_pose::foot_set_location(
        &mut sim.substrate.entities,
        unit,
        moved,
        &rules,
        &sim.interner,
    );
    sim.session.set_binary_frame_for_test(2);
    sim.update_alpha_shape(unit, ObjectKind::Entity, &rules);
    assert_eq!(glows(&sim, &rules)[0].0, born, "an even frame leaves it");
    sim.session.set_binary_frame_for_test(3);
    sim.update_alpha_shape(unit, ObjectKind::Entity, &rules);
    let [(after, _)] = glows(&sim, &rules)[..] else {
        panic!("one glow");
    };
    let (x, y) = crate::util::lepton::absolute_leptons_to_screen(moved.x, moved.y, 0);
    assert_eq!(
        after,
        [x as i32 - 30, y as i32 - 15],
        "re-placed from GetCoords"
    );
    assert_ne!(after, born);
}

/// Ares draws a Techno's PrimaryFacing frame (`0x0042146E`/`0x0042152C`,
/// `100431A0`), skips it while cloaked (`ShouldDraw`, `100477D0`), and
/// `UpdateAlphaShape` deletes it while the cloak holds (`10009F40`).
#[test]
fn ares_draws_the_facing_frame_and_drops_a_cloaked_technos_glow() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=ODIN\n\
         [ODIN]\nAlphaImage=ODINSH\nStrength=100\nImmuneToAbduction=yes\n",
    ))
    .unwrap();
    assert!(rules.ares());
    rules.supply_alpha_image_for_test(
        "ODINSH",
        AlphaImage::frames_for_test([60, 30], vec![([0, 0, 1, 1], vec![254]); 8]),
    );
    let entities = crate::map::entities::parse_map_entities(
        &IniFile::from_str("[Units]\n0=Americans,ODIN,256,4,4,0,None,None,None,-1,0\n"),
        4,
    );
    let mut sim = Simulation::with_seed(5);
    sim.session.set_map_width_for_test(10);
    sim.session.set_map_height_for_test(10);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    assert_eq!(sim.spawn_from_map(&entities, Some(&rules)), 1);
    let unit = sim.substrate.entities.values().next().unwrap().stable_id();
    let frame = sim.session.binary_frame();
    let entity = sim.substrate.entities.get_mut(unit).unwrap();
    entity.body_facing.set(0x4000, frame);
    entity.body_facing.set_rot(0);
    let draws: Vec<_> = sim.alpha_shape_draws(&rules).collect();
    assert_eq!(draws.len(), 1);
    // Eight frames: Current >> (16 - 3).
    assert_eq!(draws[0].frame, 2);
    // The picker reads Ares' own Current (`0x10009CC0`): on the first frame
    // of a ROT 5 turn from 0x4000 to 0 it answers 0x4000, frame 2, where
    // gamemd's answers 0x3FFC, frame 1 (`facing_class_ares.json`).
    let entity = sim.substrate.entities.get_mut(unit).unwrap();
    entity.body_facing.set_rot(5);
    entity.body_facing.snap(0x4000, frame);
    entity.body_facing.set(0, frame);
    assert_eq!(sim.alpha_shape_draws(&rules).next().unwrap().frame, 2);
    let entity = sim.substrate.entities.get_mut(unit).unwrap();
    entity.body_facing.snap(0x4000, frame);
    entity.body_facing.set_rot(0);

    let entity = sim.substrate.entities.get_mut(unit).unwrap();
    let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0);
    cloak.set_state_for_test(2);
    entity.cloak = Some(Box::new(cloak));
    assert!(glows(&sim, &rules).is_empty(), "ShouldDraw skips it");
    assert_eq!(sim.alpha_shapes.shapes.len(), 1);
    sim.session.set_binary_frame_for_test(2);
    sim.update_alpha_shape(unit, ObjectKind::Entity, &rules);
    assert!(sim.alpha_shapes.shapes.is_empty(), "withheld on any frame");
}

/// An Anim's Unlimbo glow (`0x004222E0`, `0x005F5045`). In stock the Anim's
/// expiry reaches only the Anim listeners (`0x00725A16`), which the glow
/// never joins, so it outlives the Anim. Under Ares it joins them
/// (`AlphaShapeClass_CTOR_Anims`, `0x00420A71`) and goes with the Anim.
#[test]
fn an_anims_glow_outlives_it_in_stock_but_not_under_ares() {
    for ares in [false, true] {
        let art_ini = IniFile::from_str("[FLARE]\nAlphaImage=GLOW\n");
        let ares_key = if ares {
            "[VehicleTypes]\n0=TANK\n[TANK]\nImmuneToAbduction=yes\n"
        } else {
            ""
        };
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(&format!("[Animations]\n0=FLARE\n{ares_key}")),
            &art_ini,
        )
        .unwrap();
        assert_eq!(rules.ares(), ares);
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
        art.bind_anim_frame_count_for_test("FLARE", 10);
        rules.install_art_fixture(art);
        rules.supply_alpha_image_for_test(
            "GLOW",
            AlphaImage::for_test([61, 31], [0, 0, 1, 1], vec![254]),
        );
        let mut sim = Simulation::with_seed(5);
        let flare = sim.interner.intern("FLARE");
        let zero = crate::util::fixed_math::SIM_ZERO;
        let anim = sim
            .spawn_anim_at_world(
                &rules,
                crate::sim::components::AnimClassSpawnDescriptor::new(flare, 4, 4, zero, zero, 0),
                crate::sim::anim_class::AnimWorldCoord {
                    x: 1152,
                    y: 1152,
                    z: 0,
                },
            )
            .unwrap();
        assert_eq!(glows(&sim, &rules), [([-30, 135], "GLOW")], "ares {ares}");

        sim.destroy_anim(anim, &rules);
        sim.alpha_shapes.purge_disabled();
        assert_eq!(glows(&sim, &rules).len(), usize::from(!ares), "ares {ares}");
    }
}

/// Unlimbo refuses 0,0,0 before its glow block (`0x005F4ECD`), so a Particle
/// made there casts none.
#[test]
fn nothing_made_at_the_origin_casts_a_glow() {
    use crate::sim::particles::Particle;
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[Particles]\n0=SPARK\n[SPARK]\nAlphaImage=GLOW\n",
    ))
    .unwrap();
    rules.supply_alpha_image_for_test(
        "GLOW",
        AlphaImage::for_test([61, 31], [0, 0, 1, 1], vec![254]),
    );
    let mut sim = Simulation::with_seed(5);
    for coord in [glam::IVec3::ZERO, glam::IVec3::new(1152, 1152, 0)] {
        let type_id = crate::rules::particle_type::ParticleTypeId(0);
        let mut particle = Particle::test_fixture(type_id, coord);
        particle.set_stable_id_for_test(sim.substrate.allocate_stable_id());
        sim.unlimbo_particle_alpha_shape(&particle, &rules);
    }
    assert_eq!(glows(&sim, &rules), [([-30, 135], "GLOW")]);
}

/// Deleting a system at once runs each particle's destructor, whose expiry
/// (`0x0062D9C8`) disables the particle's glow for PurgeDisabled. Stock rules.
#[test]
fn a_particle_glow_goes_when_its_system_is_deleted_at_once() {
    use crate::sim::particles::{Particle, ParticleSystem};
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[Particles]\n0=SPARK\n[SPARK]\nAlphaImage=GLOW\n\
         [ParticleSystems]\n0=SYS\n[SYS]\nHoldsWhat=SPARK\n",
    ))
    .unwrap();
    rules.supply_alpha_image_for_test(
        "GLOW",
        AlphaImage::for_test([61, 31], [0, 0, 1, 1], vec![254]),
    );
    let type_id = crate::rules::particle_type::ParticleTypeId(0);
    let mut sim = Simulation::with_seed(5);
    let mut particle = Particle::test_fixture(type_id, glam::IVec3::new(1152, 1152, 0));
    particle.set_stable_id_for_test(sim.substrate.allocate_stable_id());
    sim.unlimbo_particle_alpha_shape(&particle, &rules);
    assert_eq!(glows(&sim, &rules), [([-30, 135], "GLOW")]);

    let system_id = sim.substrate.allocate_stable_id();
    let mut system = ParticleSystem::test_fixture(
        system_id,
        crate::rules::particle_system_type::ParticleSystemTypeId(0),
        glam::IVec3::new(1152, 1152, 0),
    );
    system.set_particles_for_test(vec![particle]);
    sim.substrate.particle_systems.insert(system);
    sim.delete_particle_system_now(system_id, crate::sim::world::UninitContext::new(&rules));
    sim.alpha_shapes.purge_disabled();
    assert!(glows(&sim, &rules).is_empty());
}
