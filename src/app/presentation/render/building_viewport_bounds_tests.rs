//! Physical RushHr building through the production SHP atlas and emitter.
//! Component admission and pixels through the actual lowerer, BUILDNGZ, SHP
//! dispatcher and wheel-zoom pass. This is not a native whole-Display capture.

use super::draw_plan_lowering::{NativeDisplayOrder, lower_object_instances};
use super::merge_passes::{ObjectDrawPhase, draw_native_object_pass};
use crate::app::presentation::instances::{ShpInputs, build_shp_instances_from_inputs};
use crate::app::presentation::interpolation::RenderInterpolation;
use crate::map::{houses::HouseColorMap, lighting::CellLightGrid};
use crate::render::batch::{BatchRenderer, DepthAxis, InstanceBufferPool};
use crate::render::sprite_atlas::build_sprite_atlas;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear};
use crate::render::unit_atlas::BarrelImagePitches;
use crate::rules::house_colors::NO_REMAP;
use std::cell::RefCell;

#[test]
#[ignore = "requires GPU and physical retail RushHr assets; production building viewport admission and raster"]
fn retail_rushhr_roof_below_viewport_keeps_its_visible_body() {
    let Some((root, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let scene = crate::headless_scenario::load(&root, "RushHr.map", 31)
        .expect("ordinary stock Battle map, rules and map placements");
    assert_eq!(scene.map.header.theater, "NEWURBAN");
    let theater = crate::map::theater::load_theater(&mut assets, "NEWURBAN").unwrap();
    let sim = &scene.runtime.simulation;
    let rules = &scene.runtime.resources.rules;
    let building = sim
        .entities()
        .values()
        .find(|entity| {
            sim.interner().resolve(entity.type_ref()) == "CANEWY06"
                && (entity.position.rx(), entity.position.ry()) == (117, 75)
        })
        .expect("the stock map's healthy Wall Street office");
    assert_eq!(
        building.health.current(),
        rules.object("CANEWY06").unwrap().strength
    );
    assert!(building.passenger_role.occupants().is_empty());
    let interpolation = RenderInterpolation::new(false);
    let pose = interpolation.sample_entity(building, sim.session().binary_frame());
    let anchor =
        crate::render::locomotor_visual::building_art_anchor(pose.screen[0], pose.screen[1]);
    assert_eq!(anchor, (1260., 2895.));

    let gpu = Gpu::new();
    let mut batch = BatchRenderer::new_with_device(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let world_zoom = crate::render::upscale_pass::WorldZoomPass::new(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        [1024, 768],
        4.,
        false,
    );
    let colors = HouseColorMap::from([("Neutral".to_owned(), NO_REMAP)]);
    // Only this physical building's image is needed, while its simulation
    // still comes from the complete unmodified retail scenario constructor.
    let atlas = build_sprite_atlas(
        &gpu.device,
        &gpu.queue,
        &batch,
        &crate::sim::entity_store::EntityStore::new(),
        &assets,
        &theater.unit_palette,
        "ubn",
        "NEWURBAN",
        Some(rules),
        Some(rules.art()),
        &colors,
        &["CANEWY06"],
        &Default::default(),
        &Default::default(),
        &Default::default(),
        None,
        None,
        Some(sim.interner()),
    )
    .expect("the selected NEWURBAN physical building SHP");
    let (_, body) = atlas
        .entries_iter()
        .find(|(key, _)| key.type_id == "CANEWY06" && key.frame == 0)
        .expect("physical healthy body frame");
    assert_eq!(body.pixel_size, [161., 299.]);
    assert_eq!((body.offset_x, body.offset_y), (-79., -217.));

    let native = crate::test_fixture::json("tools/spatial_oracle/building_render_bounds.json");
    let control = native["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["physical_shp"] == "CNNEWY06.SHP")
        .unwrap();
    assert_eq!(
        serde_json::json!(atlas.building_type_bounds("CANEWY06").unwrap()),
        control["bounds"]
    );
    let zfile = assets
        .load_file_from_mix(crate::render::building_zshape::BUILDNGZ_FILE)
        .expect("native Building z-shape source");
    let zshape = crate::assets::shp_file::ShpFile::from_bytes(&zfile.bytes).unwrap();
    let (width, height, texels) =
        crate::render::building_zshape::decode_zshape_canvas(&zshape).unwrap();
    let zview = crate::render::batch::create_unorm_texture_view_on_device(
        &gpu.device,
        &gpu.queue,
        "Retail building bounds ZShape",
        &texels,
        width,
        height,
        wgpu::TextureFormat::R8Unorm,
    );
    let zbind = batch.create_zshape_bind_group_on_device(&gpu.device, &zview);
    let lighting = CellLightGrid::new();
    let pitches = RefCell::new(BarrelImagePitches::default());
    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let before = (sim.state_hash(), sim.rng_state());
    let mut parents = Vec::new();
    build_shp_instances_from_inputs(
        &ShpInputs {
            runtime: &scene.runtime,
            atlas: Some(&atlas),
            overlay_atlas: None,
            unit_atlas: None,
            barrel_image_pitches: &pitches,
            interpolation: &interpolation,
            camera: [1160., 2502.],
            render_size: [1024, 768],
            zoom: 4.,
            building_viewport: super::tactical_world_viewport(
                [1160., 2502.],
                [0, 0, 856, 736],
                Some(&world_zoom),
            ),
            axis: DepthAxis::NONE,
            local_owner: None,
            ignore_visibility: true,
            lighting: &lighting,
            house_colors: &colors,
            building_zshape: true,
        },
        &mut parents,
        &order,
    );
    let parent = parents
        .into_iter()
        .find(|parent| parent.parent.id == building.stable_id())
        .expect("the roof's eight opaque rows intersect the tactical viewport");
    assert!(!parent.building_pieces.as_ref().unwrap().is_empty());
    assert_eq!((sim.state_hash(), sim.rng_state()), before);

    let size = world_zoom.source_size();
    assert_eq!(size, [256, 192]);
    let mut view = camera(size);
    view.camera_pos = [1160., 2502.];
    view.world_origin_y = 0.;
    view.world_height = 1.;
    batch.write_camera(&gpu.queue, view);
    let fog = crate::render::shroud_buffer::ShroudBuffer::fixture(
        &gpu.device,
        &gpu.queue,
        size,
        view.camera_pos,
        &vec![127; size[0] as usize * size[1] as usize],
    );
    batch.bind_shroud(&gpu.device, Some(&fog));
    let layer_index = parent.parent.layer.0 as usize;
    let layers = lower_object_instances(vec![parent]);
    let layer = &layers[layer_index];
    let mut pool = InstanceBufferPool::new();
    pool.upload_on_device(&gpu.device, &gpu.queue, "roof", &layer.instances);
    let color = gpu.target(size, wgpu::TextureFormat::Bgra8UnormSrgb);
    let color_view = color.create_view(&Default::default());
    let mut terrain = TerrainDrawRenderer::new(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        &batch,
    );
    terrain.prepare(
        &gpu.device,
        &color,
        world_zoom.depth_view(),
        batch.camera_uniform(),
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let background = wgpu::Color {
        r: 1.,
        g: 0.,
        b: 1.,
        a: 1.,
    };
    clear(
        &mut encoder,
        &color_view,
        world_zoom.depth_view(),
        u16::MAX,
        wgpu::LoadOp::Clear(background),
    );
    for phase in [ObjectDrawPhase::BuildingBase, ObjectDrawPhase::Display] {
        draw_native_object_pass(
            &mut encoder,
            phase,
            &color_view,
            world_zoom.depth_view(),
            &mut terrain,
            world_zoom.world_clip([0, 0, 856, 736]),
            &batch,
            pool.get("roof"),
            layer,
            None,
            None,
            None,
            Some(&atlas),
            None,
            &zbind,
            None,
            None,
        );
    }
    world_zoom.composite(
        &gpu.device,
        &mut encoder,
        &color,
        [0, 0, 856, 736],
        background,
    );
    let read = gpu.read(&mut encoder, world_zoom.color_texture());
    let pixels = gpu.finish(encoder, &[read], [1024, 768]).remove(0);
    let pixel = |x: usize, y: usize| -> [u8; 4] {
        pixels[(y * 1024 + x) * 4..(y * 1024 + x + 1) * 4]
            .try_into()
            .unwrap()
    };
    // The first two literal index71 texels in the real crop occupy two4x4
    // blocks above the actual bottom strip, after the original Z-shape clip.
    for y in 704..708 {
        for x in 404..412 {
            assert_ne!(pixel(x, y), [255, 0, 255, 255], "roof at{x},{y}");
        }
    }
    assert_eq!(
        pixel(404, 736),
        [255, 0, 255, 255],
        "bottom strip stays clipped"
    );
}
