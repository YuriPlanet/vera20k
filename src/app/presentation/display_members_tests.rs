use super::*;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::combat::Health;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::game_entity::GameEntity;

/// A unit is filed as a record in its frame slot, an Anim as an effect, and
/// a Terrain object, which neither loop draws, is left out; a second ask
/// returns the first table.
#[test]
fn each_display_member_is_filed_under_its_store_and_terrain_is_left_out() {
    let rules_ini = IniFile::from_str("[Animations]\n0=FIRE\n");
    let art_ini = IniFile::from_str("[FIRE]\nEnd=20\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    rules.bind_anim_frame_count_for_test("FIRE", 20);
    let mut sim = Simulation::with_seed(31);
    let fire = sim.intern("FIRE");
    let coord = AnimWorldCoord {
        x: 16512,
        y: 16512,
        z: 0,
    };
    let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
    // The Anim first, so the ids given by hand below come after its own.
    let anim = sim
        .spawn_anim_at_world(
            &rules,
            AnimClassSpawnDescriptor::new(fire, rx, ry, sub_x, sub_y, z),
            coord,
        )
        .unwrap();
    let (tank_id, tree_id) = (anim + 1, anim + 2);
    let owner = sim.intern("Americans");
    let tank = sim.intern("MTNK");
    sim.entities_mut_for_test()
        .insert(GameEntity::new_at_frame_zero_for_test(
            tank_id,
            20,
            20,
            0,
            0,
            owner,
            Health::new(100),
            tank,
            EntityCategory::Unit,
            0,
            5,
            false,
        ));
    let tree = sim.intern("TREE01");
    sim.terrain_objects_mut_for_test()
        .objects_mut_for_test()
        .insert(
            tree_id,
            crate::sim::terrain_object::TerrainObjectState::for_test(tree_id, tree, 12, 12),
        );
    for id in [tank_id, tree_id] {
        sim.submit_object_display_for_test(id, DisplayLayer::GROUND, Some(&rules));
    }
    let frame = PresentationFrame::of_test(&sim, &rules);
    let members = frame.display_members(&sim);
    assert_eq!(
        members.objects(),
        [ObjectMember {
            layer: DisplayLayer::GROUND,
            id: tank_id,
            kind: ObjectKind::Record {
                slot: 0,
                span: ScreenSpan::of(frame.object(tank_id).unwrap()),
            },
        }]
    );
    // The Anim is filed with the point it draws at.
    assert!(
        matches!(
            members.effects(),
            [EffectMember { id, kind: EffectKind::Anim(Some(_)) }] if *id == anim
        ),
        "{:?}",
        members.effects()
    );
    assert!(std::ptr::eq(members, frame.display_members(&sim)));
}

/// An aircraft's shadow, drawn on the ground below it, lies in its span for
/// Fly and for Rocket, whose shadow sinks below the ground by its height:
/// the pass-by keeps an aircraft over the view's top edge whose shadow is in
/// view.
#[test]
fn an_aircraft_span_covers_its_shadow_on_the_ground() {
    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    let plane = sim.intern("ORCA");
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        30,
        30,
        0,
        0,
        owner,
        Health::new(100),
        plane,
        EntityCategory::Aircraft,
        0,
        5,
        true,
    );
    entity.position.set_exact_z_leptons_for_test(Some(1500));
    let record = ObjectRecord::of_test(&entity);
    let span = ScreenSpan::of(&record);
    let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&record.position);
    for locomotor in [LocomotorKind::Fly, LocomotorKind::Rocket] {
        let shadow = crate::render::aircraft_shadow::pose(
            crate::render::aircraft_shadow::AircraftShadowContext {
                xyz: [x, y, record.place.world_z_leptons],
                on_bridge: false,
                locomotor,
                is_dropship: false,
                primary_facing: 0,
                secondary_facing: 0,
            },
            |coord| crate::render::foot_depth::DepthCell {
                coord,
                ..Default::default()
            },
        )
        .unwrap();
        let [shadow_x, shadow_y] = shadow.anchor;
        assert!(span.top < shadow_y, "{locomotor:?}: {span:?} {shadow_y}");
        assert!(
            shadow_y <= span.bottom,
            "{locomotor:?}: {span:?} {shadow_y}"
        );
        assert!((shadow_x - span.x).abs() <= 1.0, "{locomotor:?}");
    }
}
