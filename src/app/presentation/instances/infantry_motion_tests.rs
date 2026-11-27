//! The emitted retail SHP body, its frame and its shadow on a middle draw.
use super::*;
use crate::app::presentation::frame::PresentationFrame;
use crate::app::presentation::unit_motion::UnitMotion;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteEntry, SpriteAtlas};
use crate::sim::components::DriveCoord;
use crate::sim::runtime::{SimResources, SimRuntime};

fn draw(
    runtime: &SimRuntime,
    id: u64,
    kind: &str,
    motion: Option<&UnitMotion>,
) -> (SpriteInstance, SpriteInstance) {
    let frame = PresentationFrame::capture(runtime, &Default::default(), false);
    let record = frame.object(id).unwrap();
    let current = resolve_object_shp_frame_with_facing(
        Some(runtime),
        Some(&runtime.resources.rules),
        kind,
        record,
        record.pose.body,
        None,
    )
    .unwrap()
    .0;
    let entry = ShpSpriteEntry {
        uv_origin: [f32::from(current) / 4096.0, 0.0],
        uv_size: [0.02; 2],
        pixel_size: [10.0, 16.0],
        offset_x: -5.0,
        offset_y: -12.0,
        canvas_rect: [0.0, 0.0, 10.0, 16.0],
        extended: false,
        page: 0,
        source_palette: 0,
    };
    let key = ShpSpriteKey {
        palette_context: ShpPaletteContext::Legacy,
        type_id: kind.into(),
        facing: 0,
        frame: current,
        house_color: HouseColorIndex(0),
    };
    let shadow_key = if kind == "JUMPJET" {
        ShpSpriteKey {
            palette_context: ShpPaletteContext::GlobalAnim,
            type_id: "INFSHDW".into(),
            frame: 1,
            ..key.clone()
        }
    } else {
        ShpSpriteKey {
            frame: current + 2048,
            ..key.clone()
        }
    };
    let mut atlas = SpriteAtlas::from_test_entries(
        [
            (key, entry),
            (
                shadow_key,
                ShpSpriteEntry {
                    page: 1,
                    offset_x: -2.0,
                    offset_y: -2.0,
                    ..entry
                },
            ),
        ]
        .into_iter()
        .collect(),
    );
    atlas.set_object_shp_frame_count_for_test(kind, 4096);
    let camera = [-100.0, 0.0];
    let input = ShpInputs {
        motion,
        diagnostics: None,
        runtime,
        frame: &frame,
        atlas: Some(&atlas),
        overlay_atlas: None,
        unit_atlas: None,
        voxel_images: &Default::default(),
        zshape_skips: &Default::default(),
        depth_tested_images: None,
        camera,
        render_size: [800, 600],
        zoom: 1.5,
        building_viewport: crate::app::presentation::render::tactical_world_viewport(
            camera,
            [0, 0, 800, 600],
            None,
        ),
        axis: DepthAxis {
            origin_y: 0.0,
            world_height: 10_000.0,
        },
        local_owner: Some("Americans"),
        ignore_visibility: true,
        lighting: &crate::map::lighting::CellLightGrid::new(),
        house_colors: &crate::map::houses::HouseColorMap::from([(
            "Americans".to_owned(),
            HouseColorIndex(0),
        )]),
        building_zshape: None,
    };
    let order = NativeDisplayOrder::from_ordered(vec![(
        if kind == "JUMPJET" {
            crate::sim::world::display_layers::DisplayLayer::AIR
        } else {
            crate::sim::world::display_layers::DisplayLayer::GROUND
        },
        id,
    )]);
    let mut plan = PlannedObjects::default();
    shp_visitor(&input, &order)(ShpDraw::Member(&mut plan), record);
    let parts = plan
        .into_owned()
        .pop()
        .expect("native infantry body")
        .pieces;
    let body = parts
        .iter()
        .find(|piece| matches!(piece.target, ObjectTexture::ShpPage(0)))
        .unwrap()
        .instance;
    let shadow = parts
        .iter()
        .find(|piece| matches!(piece.target, ObjectTexture::SpriteAtlasEdit(1, _)))
        .unwrap()
        .instance;
    (body, shadow)
}

#[test]
fn walking_gi_and_climbing_rocketeer_move_the_body_and_shadow_on_the_committed_shp_frame() {
    for (kind, doing, height) in [("E1", 3, 0), ("JUMPJET", 24, 128)] {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        else {
            return;
        };
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let id = sim
            .spawn_object(kind, "Americans", 10, 10, 0, &rules)
            .unwrap();
        let mut runtime = SimRuntime {
            simulation: sim,
            resources: SimResources::new(rules).into(),
        };
        let at = DriveCoord::cell(10, 10, height);
        let set = |rt: &mut SimRuntime, point, stage, facing| {
            let infantry = rt.simulation.entity_mut_for_test(id).unwrap();
            infantry.position.put_location(point);
            infantry
                .mission_leaf
                .set_infantry_doing_verified(doing)
                .unwrap();
            infantry.set_native_stage_value(stage);
            infantry.body_facing.snap(facing, 0);
        };
        set(&mut runtime, at, 0, 0);
        let mut motion = UnitMotion::default();
        motion.begin_draw(3, Some(1.0), 1.5);
        let (from, from_shadow) = draw(&runtime, id, kind, Some(&motion));
        motion.commit_presented();
        set(
            &mut runtime,
            DriveCoord {
                x: at.x + 96,
                y: at.y + 48,
                z: height * 2,
            },
            1,
            0x4000,
        );
        let (to, to_shadow) = draw(&runtime, id, kind, None);
        motion.begin_draw(4, Some(0.5), 1.5);
        let (body, shadow) = draw(&runtime, id, kind, Some(&motion));
        let midpoint = |a: [f32; 2], b: [f32; 2]| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        assert_ne!(from.position, to.position);
        assert_eq!(
            body.position,
            midpoint(from.position, to.position),
            "{kind} body"
        );
        assert_eq!(
            shadow.position,
            midpoint(from_shadow.position, to_shadow.position),
            "{kind} shadow"
        );
        assert_eq!(body.uv_origin, to.uv_origin, "{kind} exact native frame");
        assert_ne!(
            from.uv_origin, to.uv_origin,
            "{kind} fixture advances native art"
        );
        motion.commit_presented();
        assert_eq!(
            motion.painted_anchor(id),
            Some([body.position[0] + 5.0, body.position[1] + 12.0])
        );
    }
}

/// A Rocketeer flies across the width of a high bridge. Its committed
/// position is over the deck while the half-step still casts onto the ground.
#[test]
fn rocketeer_shadow_uses_the_bridge_under_the_displayed_position() {
    use crate::map::bridge_facts::{BRIDGE_FLAG_DIRECTION_ZERO, BRIDGE_FLAG_STRUCTURAL};

    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let id = sim
        .spawn_object("JUMPJET", "Americans", 9, 10, 0, &rules)
        .unwrap();
    let terrain = sim.resolved_terrain_mut_for_test().as_mut().unwrap();
    for y in [9, 10] {
        terrain
            .cell_mut_for_test(10, y)
            .unwrap()
            .bridge_facts
            .raw_flags |= BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DIRECTION_ZERO;
    }
    sim.entity_mut_for_test(id)
        .unwrap()
        .mission_leaf
        .set_infantry_doing_verified(24)
        .unwrap();
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: SimResources::new(rules).into(),
    };
    let mut motion = UnitMotion::default();
    let shadow = |rt: &mut SimRuntime, dx, motion: Option<&UnitMotion>| {
        // Constant altitude and world X+Y keep the ground row fixed.
        let point = DriveCoord {
            x: 10 * 256 + dx,
            y: 10 * 256 + 96 - dx,
            z: 600,
        };
        rt.simulation
            .entity_mut_for_test(id)
            .unwrap()
            .position
            .put_location(point);
        let sprite = draw(rt, id, "JUMPJET", motion).1;
        (sprite.position, sprite.z_adjust)
    };
    let ground = shadow(&mut runtime, -32, None);
    let deck = shadow(&mut runtime, 32, None);
    assert_ne!(
        ground.1, deck.1,
        "the native controls must land on different surfaces"
    );
    motion.begin_draw(3, Some(1.0), 1.0);
    shadow(&mut runtime, -96, Some(&motion));
    motion.commit_presented();
    motion.begin_draw(4, Some(0.5), 1.0);
    assert_eq!(shadow(&mut runtime, 32, Some(&motion)), ground);
}
