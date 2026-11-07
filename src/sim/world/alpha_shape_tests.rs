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
    sim.detach_all_pointer_expired(lamp, &rules, None, FrameEffects::default());
    assert_eq!(sim.alpha_shapes().count(), 1);
    sim.advance_tick(&[], Some(&rules), None, None, 33);
    assert_eq!(sim.alpha_shapes().count(), 0);
}
