//! A spawned particle's `AlphaImage=` glow: `ObjectClass::Unlimbo @ 0x005F4EC0`
//! constructs one at `0x005F5045..0x005F5100` for any object whose type carried
//! a loaded AlphaImage, and `ParticleTypeClass::ReadINI @ 0x00644F50` reaches
//! `ObjectTypeClass::ReadINI @ 0x005F92D0`, so a particle type can carry one.

use super::*;
use crate::rules::alpha_image_assets::AlphaImage;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

fn rules(image: &str) -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Particles]\n0=GLOWP\n1=PLAINP\n\
         [GLOWP]\nAlphaImage={image}\nBehavesLike=Smoke\nMaxEC=100\nMaxDC=1\nDamage=0\n\
         [PLAINP]\nBehavesLike=Smoke\nMaxEC=100\nMaxDC=1\nDamage=0\n"
    )))
    .unwrap();
    rules.supply_alpha_image_for_test(
        "GLOW",
        AlphaImage::for_test([61, 31], [0, 0, 1, 1], vec![254]),
    );
    rules
}

#[test]
fn a_particle_type_alpha_image_glows_and_a_missing_one_does_not() {
    let rules = rules("GLOW");
    let glow_id = rules.p_type_id_by_name("GLOWP").unwrap();
    let plain_id = rules.p_type_id_by_name("PLAINP").unwrap();

    let mut sim = Simulation::with_seed(7);
    let _glow = Particle::new(
        glow_id,
        IVec3::new(256, 256, 0),
        IVec3::ZERO,
        rules.particle_type(glow_id),
        &mut sim,
        &rules,
    );
    assert_eq!(
        sim.alpha_shapes()
            .map(|(_, image, light_flash)| (image, light_flash))
            .collect::<Vec<_>>(),
        [("GLOW", false)],
        "a loaded AlphaImage glows at the particle's own coords"
    );

    let plain = Particle::new(
        plain_id,
        IVec3::new(512, 256, 0),
        IVec3::ZERO,
        rules.particle_type(plain_id),
        &mut sim,
        &rules,
    );
    assert_eq!(
        sim.alpha_shapes().count(),
        1,
        "a type without AlphaImage adds no shape"
    );
    assert_eq!(plain.coords(), IVec3::new(512, 256, 0));
}

#[test]
fn an_unloaded_alpha_image_adds_no_shape() {
    let rules = rules("MISSING");
    let id = rules.p_type_id_by_name("GLOWP").unwrap();
    let mut sim = Simulation::with_seed(7);
    let _particle = Particle::new(
        id,
        IVec3::new(256, 256, 0),
        IVec3::ZERO,
        rules.particle_type(id),
        &mut sim,
        &rules,
    );
    assert_eq!(sim.alpha_shapes().count(), 0);
}

#[test]
fn the_light_flash_window_marks_the_glow_it_reveals() {
    // 48A444 sets the mark before the WH.Particle system starts and 48A47E
    // clears it after, so only what the window reveals carries it.
    let rules = rules("GLOW");
    let id = rules.p_type_id_by_name("GLOWP").unwrap();
    let mut sim = Simulation::with_seed(7);
    sim.set_particle_light_flash(true);
    let _glow = Particle::new(
        id,
        IVec3::new(256, 256, 0),
        IVec3::ZERO,
        rules.particle_type(id),
        &mut sim,
        &rules,
    );
    assert_eq!(
        sim.alpha_shapes()
            .map(|(_, image, light_flash)| (image, light_flash))
            .collect::<Vec<_>>(),
        [("GLOW", true)]
    );
    sim.set_particle_light_flash(false);
    let _after = Particle::new(
        id,
        IVec3::new(512, 256, 0),
        IVec3::ZERO,
        rules.particle_type(id),
        &mut sim,
        &rules,
    );
    assert_eq!(
        sim.alpha_shapes()
            .map(|(_, _, light_flash)| light_flash)
            .collect::<Vec<_>>(),
        [true, false],
        "the mark is transient: it does not survive the window"
    );
}
