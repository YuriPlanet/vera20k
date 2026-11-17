//! Ares 3.0 AnimTypeExt (`ares-src/src/Ext/AnimType`) through AnimClass::AI.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::world::Simulation;

/// Three looping damage anims on a target: PULSE every frame, SLOW with
/// `Damage.Delay=3`, ZAPPER with `Weapon=ZAP`. `DropPodWeapon=` allocates
/// ZAP before the anims' ART bodies are read, so its Find-only read finds it.
fn scene() -> (Simulation, RuleSet) {
    let verses = "Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TARGET\n\
         [TARGET]\nStrength=100000\nArmor=light\n\
         [Animations]\n0=PULSE\n1=SLOW\n2=ZAPPER\n\
         [General]\nDropPodWeapon=ZAP\n\
         [ZAP]\nDamage=1\nWarhead=WZAP\n\
         [Warheads]\n0=WPULSE\n1=WSLOW\n2=WZAP\n3=WANIM\n\
         [WPULSE]\nCellSpread=.3\n{verses}[WSLOW]\nCellSpread=.3\n{verses}\
         [WZAP]\nCellSpread=.3\n{verses}[WANIM]\nCellSpread=.3\n{verses}"
    ));
    let art = IniFile::from_str(
        "[PULSE]\nRate=900\nEnd=40\nDamage=10\nWarhead=WPULSE\n\
         [SLOW]\nRate=900\nEnd=40\nDamage=10\nWarhead=WSLOW\nDamage.Delay=3\n\
         [ZAPPER]\nRate=900\nEnd=40\nDamage=10\nWarhead=WANIM\nWeapon=ZAP\n",
    );
    let processed = crate::rules::native_processing::RulesLayerStack::new(ini)
        .process_with_fixed_art(&art)
        .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    for name in ["PULSE", "SLOW", "ZAPPER"] {
        registry.bind_anim_frame_count_for_test(name, 40);
    }
    rules.install_art_data(registry);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x424538);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    for (x, name) in [(10, "PULSE"), (20, "SLOW"), (30, "ZAPPER")] {
        let target = sim
            .spawn_object("TARGET", "Russians", x, 20, 0, &rules)
            .unwrap();
        let coord = crate::sim::movement::ground_pose::position_world_coord(
            &sim.entities().get(target).unwrap().position,
        );
        let world = AnimWorldCoord {
            x: coord.x,
            y: coord.y,
            z: coord.z,
        };
        let (rx, ry, sx, sy, z) = world.to_cell_sub_z();
        let kind = sim.interner.intern(name);
        sim.spawn_anim_at_world(
            &rules,
            AnimClassSpawnDescriptor::new(kind, rx, ry, sx, sy, z),
            world,
        )
        .unwrap();
    }
    (sim, rules)
}

/// AnimClass_Update_DamageDelay (1000AFE0, hook 0x424538): `Damage.Delay=3`
/// lands the whole Damage once every third damage frame; `Weapon=`
/// detonates a bullet of that weapon (its Warhead) instead of the area
/// damage with the anim's own Warhead.
#[test]
fn ares_anim_damage_delay_and_weapon() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let (mut sim, rules) = scene();
    let (_, visits) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
        for _ in 0..9 {
            sim.advance_tick(&[], Some(&rules), None, Some(rules.overlay_registry()), 22);
        }
    });
    let count = |warhead: &str| visits.iter().filter(|v| v["warhead"] == warhead).count();
    let every_frame = count("WPULSE");
    assert!(every_frame >= 6, "{visits:?}");
    // The constructor seeds the +0x188 count with 1.0 (0x00421FA4), so the
    // first delivery comes on the second damage frame.
    assert_eq!(count("WSLOW"), (every_frame + 1) / 3);
    assert_eq!(count("WZAP"), every_frame);
    assert_eq!(count("WANIM"), 0);
}
