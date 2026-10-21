//! Retail PARACH atlas closure, canopy instance building, and offscreen draw.

use super::draw_plan_lowering::{
    NativeDisplayOrder, ObjectTexture, PlannedObjectInstance, lower_object_instances,
};
use super::merge_passes::draw_native_object_pass;
use crate::app::presentation::detail::DetailState;
use crate::app::presentation::instances::{DisplayEffectInputs, build_parachute_instances};
use crate::assets::pal_file::Palette;
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::map::lighting::CellLightGrid;
use crate::render::batch::{BatchRenderer, DepthAxis, InstanceBufferPool};
use crate::render::locomotor_visual::screen_position;
use crate::render::sprite_atlas::{
    ShpPaletteContext, ShpSpriteKey, anim_remap_color, build_sprite_atlas,
    collect_anim_remap_base_keys,
};
use crate::render::tactical_draw_plan::SpriteEncoding;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear};
use crate::rules::effect_asset_catalog::tolerant_anim_class_roots;
use crate::rules::house_colors::{DEFAULT_SCHEME_ENTRY, HouseColorIndex};
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::combat::Health;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::parachute_descent::begin_parachute_descent;
use crate::sim::world::Simulation;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};

#[test]
#[ignore = "requires GPU and retail RA2/YR assets; live owner remaps through the production canopy builder and sprite pass"]
fn retail_parachute_canopy_uses_each_attached_house_remap_in_gpu_pixels() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();

    let mut rules = retail.rules;
    let roots = tolerant_anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "tem", "TEMPERATE");
    let parachute_type = rules
        .general
        .parachute_shp
        .as_deref()
        .expect("retail [General] Parachute");
    assert!(
        rules
            .art()
            .anim_runtime_config(parachute_type)
            .is_some_and(|config| config.raw_shp_frame_count.is_some()),
        "the retail AnimClass binder supplies PARACH's frame count"
    );

    let scheme_colors: Vec<_> = rules
        .color_schemes
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 0 && *index != DEFAULT_SCHEME_ENTRY)
        .take(2)
        .map(|(index, _)| HouseColorIndex(u8::try_from(index).unwrap()))
        .collect();
    assert_eq!(scheme_colors.len(), 2, "two non-default retail schemes");
    assert_ne!(scheme_colors[0], scheme_colors[1]);
    let owner_names = ["CanopyOwnerOne", "CanopyOwnerTwo"];
    let house_colors: HouseColorMap = owner_names
        .into_iter()
        .zip(scheme_colors.iter().copied())
        .map(|(owner, color)| (owner.to_string(), color))
        .collect();

    let mut sim = Simulation::new();
    *sim.interner_mut_for_test() = crate::sim::intern::test_interner();
    let owner_ids = owner_names.map(|owner| sim.intern(owner));
    let infantry_type = sim.intern("E1");
    let positions = [(10, 10), (14, 14)];
    let mut body_ids = Vec::with_capacity(2);
    for (owner, (rx, ry)) in owner_ids.iter().copied().zip(positions) {
        let id = sim.substrate_mut_for_test().allocate_stable_id();
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            id,
            rx,
            ry,
            0,
            0,
            owner,
            Health::new(100),
            infantry_type,
            EntityCategory::Infantry,
            0,
            5,
            false,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        sim.entities_mut_for_test().insert(entity);
        assert!(begin_parachute_descent(
            sim.entities_mut_for_test(),
            id,
            600
        ));
        body_ids.push(id);
    }
    let parachute_anim_type = sim.intern(parachute_type);
    let mut anim_ids = Vec::with_capacity(body_ids.len());
    for (&body_id, &owner) in body_ids.iter().zip(&owner_ids) {
        let body = sim.entities().get(body_id).unwrap();
        let canopy_coord = AnimWorldCoord {
            x: i32::from(body.position.rx()) * crate::util::lepton::LEPTONS_PER_CELL_I32
                + body.position.sub_x().to_num::<i32>(),
            y: i32::from(body.position.ry()) * crate::util::lepton::LEPTONS_PER_CELL_I32
                + body.position.sub_y().to_num::<i32>(),
            z: body
                .position
                .exact_z_leptons()
                .expect("parachuted body has an exact Z")
                .wrapping_add(75),
        };
        let (rx, ry, sub_x, sub_y, level) = canopy_coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            owner: Some(body_id),
            ..AnimClassSpawnDescriptor::new(parachute_anim_type, rx, ry, sub_x, sub_y, level)
        };
        let anim_id = sim
            .spawn_anim_at_world(&rules, descriptor, canopy_coord)
            .expect("retail canopy anim");
        assert!(
            sim.anims_mut_for_test()
                .set_anim_house_remap(anim_id, owner)
        );
        anim_ids.push(anim_id);
    }
    for ((&anim_id, &owner), &color) in anim_ids.iter().zip(&owner_ids).zip(&scheme_colors) {
        let anim = sim.anim(anim_id).expect("attached canopy stays live");
        assert_eq!(
            anim.remap(),
            Some(crate::sim::anim_class::AnimRemap::House(owner))
        );
        assert_eq!(
            anim_remap_color(anim, sim.interner(), &house_colors),
            Some(color),
            "house identity resolves through the match color map"
        );
    }

    let live_remap_keys = collect_anim_remap_base_keys(&sim, &house_colors);
    let parachute_name = parachute_type.to_string();
    for color in &scheme_colors {
        assert!(live_remap_keys.contains(&(
            parachute_name.to_ascii_uppercase(),
            crate::render::sprite_atlas::ShpPaletteContext::SelectedScheme,
            *color
        )));
    }
    assert_eq!(live_remap_keys.len(), 2, "only the two live canopy remaps");

    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let palette =
        Palette::from_bytes(assets.get_ref("UNITTEM.PAL").unwrap()).expect("retail unit palette");
    let no_cell_drawers = HashSet::new();
    let atlas = build_sprite_atlas(
        &gpu.device,
        &gpu.queue,
        &batch,
        sim.entities(),
        &assets,
        &palette,
        "tem",
        "TEMPERATE",
        Some(&rules),
        Some(rules.art()),
        &house_colors,
        &[],
        &live_remap_keys,
        &no_cell_drawers,
        None,
        None,
        Some(sim.interner()),
    )
    .expect("retail E1 and live PARACH remap atlas");

    let keys_and_entries: Vec<_> = anim_ids
        .iter()
        .zip(&scheme_colors)
        .map(|(&anim_id, &color)| {
            let anim = sim.anim(anim_id).unwrap();
            let key = ShpSpriteKey {
                palette_context: ShpPaletteContext::SelectedScheme,
                type_id: parachute_name.clone(),
                facing: 0,
                frame: u16::try_from(anim.runtime().current_frame).unwrap(),
                house_color: color,
            };
            let entry = atlas
                .get(&key)
                .unwrap_or_else(|| panic!("live atlas includes {key:?}"));
            (key, *entry)
        })
        .collect();

    let anchors: Vec<_> = body_ids
        .iter()
        .map(|&id| screen_position(sim.entities().get(id).unwrap()))
        .collect();
    let size = [512, 256];
    let view_camera = [
        anchors
            .iter()
            .map(|point| point.0)
            .fold(f32::INFINITY, f32::min)
            - 64.0,
        anchors
            .iter()
            .map(|point| point.1)
            .fold(f32::INFINITY, f32::min)
            - 64.0,
    ];
    let detail = RefCell::new(DetailState::new());
    let lighting = CellLightGrid::new();
    let hidden = BTreeSet::new();
    let input = DisplayEffectInputs {
        interpolation: None,
        sim: &sim,
        rules: &rules,
        camera: view_camera,
        render_size: size,
        tactical: [0, 0, size[0], size[1]],
        zoom: 1.0,
        axis: DepthAxis::NONE,
        atlas: Some(&atlas),
        detail: &detail,
        detail_level: 2,
        lighting: &lighting,
        house_colors: &house_colors,
        hidden_super_anims: &hidden,
        show_hidden: false,
        local_owner: None,
        observe_particles: false,
    };
    let order = NativeDisplayOrder::new(&body_ids);
    let mut parents: Vec<_> = body_ids
        .iter()
        .map(|&id| {
            PlannedObjectInstance::object(
                order.object_draw(id, SpriteEncoding::Plain).unwrap(),
                Vec::new(),
            )
        })
        .collect();
    let body_depths = HashMap::from([(body_ids[0], 0.5), (body_ids[1], 0.5)]);
    build_parachute_instances(&input, &mut parents, &body_depths, None);

    let mut canopy_instances = Vec::with_capacity(2);
    for ((&id, (key, entry)), parent) in body_ids.iter().zip(&keys_and_entries).zip(&parents) {
        assert_eq!(parent.parent.id, id);
        assert_eq!(parent.pieces.len(), 1, "only the canopy is in this parent");
        let piece = &parent.pieces[0];
        assert_eq!(piece.target, ObjectTexture::ShpPage(entry.page as usize));
        assert_eq!(
            piece.instance.size, entry.pixel_size,
            "builder uses {key:?}"
        );
        assert_eq!(
            piece.instance.uv_origin, entry.uv_origin,
            "builder uses {key:?}"
        );
        assert_eq!(
            piece.instance.uv_size, entry.uv_size,
            "builder uses {key:?}"
        );
        assert_eq!(
            piece.instance.source_palette, entry.source_palette,
            "builder uses {key:?}"
        );
        canopy_instances.push(piece.instance);
    }

    let layers = lower_object_instances(parents);
    let ground = &layers[2];
    assert_eq!(ground.instances.len(), 2);
    assert_eq!(ground.owners, body_ids);
    assert!(
        ground
            .runs
            .iter()
            .all(|run| matches!(run.target, ObjectTexture::ShpPage(_)))
    );

    let mut camera_view = camera(size);
    camera_view.camera_pos = view_camera;
    batch.write_camera(&gpu.queue, camera_view);
    let mut pool = InstanceBufferPool::new();
    pool.upload_on_device(
        &gpu.device,
        &gpu.queue,
        "parachute_canopies",
        &ground.instances,
    );
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
    terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    clear(
        &mut encoder,
        &cv,
        &dv,
        u16::MAX,
        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
    );
    draw_native_object_pass(
        &mut encoder,
        crate::app::render::merge_passes::ObjectDrawPhase::Display,
        &cv,
        &dv,
        &mut terrain,
        [0, 0, size[0], size[1]],
        &batch,
        pool.get("parachute_canopies"),
        ground,
        None,
        None,
        None,
        Some(&atlas),
        None,
        batch.default_zshape_bind_group(),
        None,
        None,
    );
    let reads = [Gpu::read(&gpu, &mut encoder, &color)];
    let pixels = gpu.finish(encoder, &reads, size).remove(0);

    let rect = |instance: &crate::render::batch::SpriteInstance| {
        [
            (instance.position[0] - view_camera[0]).floor() as i32,
            (instance.position[1] - view_camera[1]).floor() as i32,
            instance.size[0].ceil() as i32,
            instance.size[1].ceil() as i32,
        ]
    };
    let [ax, ay, width, height] = rect(&canopy_instances[0]);
    let [bx, by, other_width, other_height] = rect(&canopy_instances[1]);
    assert_eq!((width, height), (other_width, other_height));
    assert_eq!(keys_and_entries[0].0.frame, keys_and_entries[1].0.frame);
    let (one_instance, two_instance) = (&canopy_instances[0], &canopy_instances[1]);
    assert_eq!(one_instance.size, two_instance.size);
    assert_eq!(one_instance.tint, two_instance.tint);
    assert_eq!(one_instance.alpha, two_instance.alpha);
    assert_eq!(one_instance.depth, two_instance.depth);
    assert_eq!(one_instance.palette_light, two_instance.palette_light);
    for (one, two) in one_instance.position.into_iter().zip(two_instance.position) {
        let delta = two - one;
        assert!(
            (delta - delta.round()).abs() < 0.0001,
            "canopies use raster-aligned translations: {delta}"
        );
    }
    let pixel_at = |x: i32, y: i32| {
        assert!(x >= 0 && y >= 0 && x < size[0] as i32 && y < size[1] as i32);
        let offset = (y as usize * size[0] as usize + x as usize) * 4;
        <[u8; 4]>::try_from(&pixels[offset..offset + 4]).unwrap()
    };
    let (mut opaque, mut different) = (0, 0);
    for y in 0..height {
        for x in 0..width {
            let one = pixel_at(ax + x, ay + y);
            let two = pixel_at(bx + x, by + y);
            assert_eq!(one[3], two[3], "same-frame canopy alpha masks match");
            if one[3] == u8::MAX && two[3] == u8::MAX {
                opaque += 1;
                different += (one[..3] != two[..3]) as usize;
            }
        }
    }
    assert!(
        opaque > 20,
        "enough corresponding canopy pixels are opaque: {opaque}"
    );
    assert!(
        different > 10,
        "different live house schemes change opaque canopy pixels: {different} of {opaque}"
    );
}
