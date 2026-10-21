//! Presentation-only continuity controls, not native interpolated pixels.
//! Real attached PARACH state and its physical frame metadata feed the actual
//! generic emitter/lowerer. Declared owner poses and the owner-identity edit
//! exercise the render cache; no whole-flight or ownership-transfer claim.

use super::*;
use crate::app::presentation::interpolation::{PointKind, PointPose};
use crate::render::sprite_atlas::ShpSpriteEntry;

fn emitted(
    scene: &Scene,
    interpolation: &RenderInterpolation,
    atlas: &SpriteAtlas,
    colors: &HouseColorMap,
    light: &CellLightGrid,
) -> crate::render::batch::SpriteInstance {
    let sim = &scene.runtime.simulation;
    let before = (sim.state_hash(), sim.rng_state());
    let mut detail = DetailState::new();
    for _ in 0..60 {
        detail.record_logic_visit();
    }
    detail.throttle_tail(0);
    let detail = RefCell::new(detail);
    let hidden = Default::default();
    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let mut parents = Vec::new();
    build_display_effect_instances(
        &DisplayEffectInputs {
            interpolation: Some(interpolation),
            sim,
            rules: &scene.runtime.resources.rules,
            camera: vera_camera(),
            render_size: SIZE,
            tactical: CLIP,
            zoom: 1.,
            axis: DepthAxis::NONE,
            atlas: Some(atlas),
            detail: &detail,
            detail_level: 2,
            lighting: light,
            house_colors: colors,
            hidden_super_anims: &hidden,
            show_hidden: false,
            local_owner: Some(sim.interner().get(OWNER).unwrap()),
            observe_particles: false,
        },
        &mut parents,
        &order,
        Default::default(),
    );
    assert_eq!(parents.len(), 1);
    assert_eq!(parents[0].parent.id, scene.canopy);
    assert_eq!(parents[0].pieces.len(), 1);
    let layer = parents[0].parent.layer.0 as usize;
    let lowered = lower_object_instances(parents);
    assert_eq!(lowered[layer].owners, [scene.canopy]);
    assert_eq!(lowered[layer].instances.len(), 1);
    assert_eq!((sim.state_hash(), sim.rng_state()), before);
    lowered[layer].instances[0]
}

fn current_pose(scene: &Scene) -> PointPose {
    let sim = &scene.runtime.simulation;
    let coord = sim.anim_absolute_coord(scene.canopy).unwrap();
    let anim = sim.anim(scene.canopy).unwrap();
    PointPose::new(
        [coord.x, coord.y, coord.z],
        anim.type_id().index(),
        anim.owner_entity(),
    )
}

#[test]
#[ignore = "requires physical retail PARACH metadata; actual emitted attached-Anim interpolation and owner identity reset"]
fn retail_parachute_emitter_interpolates_absolute_attachment_once_and_resets_owner() {
    let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules()
        .expect("retail rules required")
        .rules;
    let (_, mut assets) =
        crate::rules::retail_ini_fixture::retail_assets().expect("retail assets required");
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    rules.bind_anim_class_assets(
        &tolerant_anim_class_roots(&rules),
        &assets,
        "tem",
        "TEMPERATE",
    );
    let physical = ShpFile::from_bytes(assets.get_ref("PARACH.SHP").unwrap()).unwrap();
    let raw = &physical.frames[0];
    assert_eq!(rules.art().anim_runtime_config("PARACH").unwrap().start, 0);
    let entry = ShpSpriteEntry {
        uv_origin: [0., 0.],
        uv_size: [1., 1.],
        pixel_size: [f32::from(raw.frame_width), f32::from(raw.frame_height)],
        offset_x: f32::from(raw.frame_x) - f32::from(physical.width / 2),
        offset_y: f32::from(raw.frame_y) - f32::from(physical.height / 2),
        canvas_rect: [
            -f32::from(physical.width / 2),
            -f32::from(physical.height / 2),
            f32::from(physical.width),
            f32::from(physical.height),
        ],
        extended: false,
        page: 0,
        source_palette: 1,
    };
    let atlas = SpriteAtlas::from_test_entries(std::collections::HashMap::from([(
        ShpSpriteKey {
            palette_context: ShpPaletteContext::SelectedScheme,
            type_id: "PARACH".into(),
            facing: 0,
            frame: 0,
            house_color: HouseColorIndex(0),
        },
        entry,
    )]));
    let colors = HouseColorMap::from([(OWNER.to_owned(), HouseColorIndex(0))]);
    let light = build_cell_light_grid_from_heights(
        [((10, 20), 0), ((11, 20), 0)],
        &LightingConfig::default(),
    );
    let mut scene = Scene::new(rules, 104);
    let first = current_pose(&scene).current();
    assert_eq!(first.coord, [2688, 5248, 104 + 75]);
    let mut interpolation = RenderInterpolation::new(true);
    interpolation.observe(&scene.runtime.simulation, 0, Some(32.), false);
    scene
        .runtime
        .simulation
        .entities_mut_for_test()
        .get_mut(scene.body)
        .unwrap()
        .position
        .put_location(DriveCoord {
            x: 2816,
            y: 5248,
            z: 168,
        });
    scene
        .runtime
        .simulation
        .session_mut_for_test()
        .set_binary_frame_for_test(1001);
    interpolation.observe(&scene.runtime.simulation, 32, Some(32.), false);
    interpolation.observe(&scene.runtime.simulation, 48, Some(32.), false);
    let second_pose = current_pose(&scene);
    let second = second_pose.current();
    assert_eq!(second.coord, [2816, 5248, 168 + 75]);
    let midpoint = interpolation.sample_point(PointKind::Anim, scene.canopy, second_pose, 1001);
    assert_eq!(midpoint.coord, [2752, 5248, 136 + 75]);
    assert_eq!(
        midpoint.screen,
        std::array::from_fn(|i| (first.screen[i] + second.screen[i]) * 0.5)
    );
    let sprite = emitted(&scene, &interpolation, &atlas, &colors, &light);
    assert_eq!(
        sprite.position,
        [
            midpoint.screen[0] + entry.offset_x,
            midpoint.screen[1] + entry.offset_y
        ],
        "sample the absolute attached Anim once, without another entity-height correction"
    );
    assert_ne!(
        sprite.position,
        [
            second.screen[0] + entry.offset_x,
            second.screen[1] + entry.offset_y
        ]
    );

    // Explicit cache identity control. Keep the step below teleport rejection
    // distance, so the owner change—not a large jump—must discard old history.
    // This edits only the declared presentation input; it is not a real game
    // SetOwner operation or a test of that separate lifecycle transaction.
    let other = scene
        .runtime
        .simulation
        .spawn_object("E1", OWNER, 11, 20, 0, &scene.runtime.resources.rules)
        .unwrap();
    scene
        .runtime
        .simulation
        .entities_mut_for_test()
        .get_mut(other)
        .unwrap()
        .position
        .put_location(DriveCoord {
            x: 2880,
            y: 5248,
            z: 200,
        });
    scene
        .runtime
        .simulation
        .anims_mut_for_test()
        .get_mut(scene.canopy)
        .unwrap()
        .set_owner_entity_for_test(Some(other));
    scene
        .runtime
        .simulation
        .session_mut_for_test()
        .set_binary_frame_for_test(1002);
    interpolation.observe(&scene.runtime.simulation, 64, Some(32.), false);
    interpolation.observe(&scene.runtime.simulation, 80, Some(32.), false);
    let changed = current_pose(&scene);
    let snapped = interpolation.sample_point(PointKind::Anim, scene.canopy, changed, 1002);
    assert_eq!(snapped.coord, [2880, 5248, 200 + 75]);
    assert_eq!(snapped.screen, changed.current().screen);
    let sprite = emitted(&scene, &interpolation, &atlas, &colors, &light);
    assert_eq!(
        sprite.position,
        [
            snapped.screen[0] + entry.offset_x,
            snapped.screen[1] + entry.offset_y
        ]
    );
}
