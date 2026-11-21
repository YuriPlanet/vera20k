use super::*;
use crate::render::unit_atlas::UnitAtlas;
use crate::render::unit_pose_cache::VxlPoseFrameCache;
use crate::render::voxel_image_cache::{VoxelCache, VoxelImageCache, VoxelImageKey};

fn alternate_voxel_style_rules() -> crate::rules::ruleset::RuleSet {
    use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile, ruleset::RuleSet};
    let art =
        IniFile::from_str("[SCHP]\nVoxel=yes\n[PLAIN]\nUseBuffer=no\n[BUFFERED]\nUseBuffer=yes\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[VehicleTypes]\n0=PLAIN\n1=BUFFERED\n2=MINER\n\
             [PLAIN]\nImage=SCHP\nStrength=100\nIsSimpleDeployer=yes\nUnloadingClass=BUFFERED\n\
             [BUFFERED]\nImage=SCHP\nStrength=100\nIsSimpleDeployer=yes\nUnloadingClass=PLAIN\n\
             [MINER]\nImage=SCHP\nStrength=100\nHarvester=yes\nUnloadingClass=BUFFERED\n",
        ),
        &art,
    )
    .unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules
}

/// Stock73B4D8 selects UnloadingClass art, while706F02 reads the actual
/// drawer's UseBuffer. A hit retains the first draw's style in either case.
#[test]
fn alternate_voxel_models_keep_the_first_drawers_raster_style() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let rules = alternate_voxel_style_rules();
    let mut sim = crate::sim::world::Simulation::new();
    let mut actors = Vec::new();
    for (name, x) in [("PLAIN", 10), ("BUFFERED", 12)] {
        let id = sim
            .spawn_object(name, "Americans", x, 10, 0, &rules)
            .unwrap();
        sim.entity_mut_for_test(id)
            .unwrap()
            .set_unit_simple_deploy_for_test(true, false, false);
        actors.push((name, id));
    }
    let mut atlas = None;
    crate::render::unit_atlas::admit_world_models(
        &mut atlas,
        sim.entities(),
        &assets,
        Some(&rules),
        Some(sim.interner()),
        8192,
    );
    let atlas = atlas.unwrap();
    let reference =
        crate::render::unit_atlas::UnitModel::load(&assets, "PLAIN", Some(&rules)).unwrap();
    let vpl = assets
        .get_ref("VOXELS.VPL")
        .and_then(|data| crate::assets::vpl_file::VplFile::from_bytes(data).ok());
    for ((actual, id), (selected, depth_tested)) in actors
        .into_iter()
        .zip([("BUFFERED", false), ("PLAIN", true)])
    {
        let record = crate::app::presentation::frame::ObjectRecord::of_test_sim(&sim, &rules, id);
        let (model, _) = unit_body_draw(
            &record,
            sim.interner(),
            Some(&rules),
            EntityDrawBand::Ground,
            sim.session().binary_frame(),
            crate::render::draw_state::ObserverDrawContext::default(),
            None,
        )
        .unwrap();
        assert_eq!(model, selected);
        let key = UnitSpriteKey {
            type_id: model.as_ref().into(),
            turret_index: 0,
            facing: 8,
            layer: VxlLayer::Composite,
            frame: 1,
            slope_type: 0,
            barrel_pitch: 0,
        };
        let expected = if depth_tested {
            reference.render_depth_tested(&key, vpl.as_ref(), None)
        } else {
            reference.render(&key, vpl.as_ref(), None, &mut None)
        }
        .unwrap()
        .0;
        let pose_cache = std::cell::RefCell::new(VxlPoseFrameCache::default());
        let live = DepthTestedImages {
            pose_cache: &pose_cache,
            assets: &assets,
            rules: Some(&rules),
        };
        let image = VoxelImageKey::new(VoxelCache::Body, key.clone());
        let mut images = VoxelImageCache::default();
        let draw = |images: &mut VoxelImageCache, drawer| {
            cached_part_entry(
                images,
                &atlas,
                Some(&live),
                drawer,
                Some(image.clone()),
                &key,
            )
            .unwrap()
        };
        let first = draw(&mut images, VoxelDrawer::of(Some(&rules), actual));
        assert!(!matches!(first.1, UnitTextureSource::Stable(_)), "{actual}");
        assert_eq!(
            pose_cache.borrow().texels(first.0),
            expected.palette_indices
        );
        assert_eq!(images.hit(&image).unwrap().depth_tested, depth_tested);
        assert_eq!(
            draw(&mut images, VoxelDrawer::of(Some(&rules), actual)),
            first
        );
        // The selected model's native cache is shared by actual types. Its
        // existing image keeps the first miss's style even for another drawer.
        assert_eq!(
            draw(&mut images, VoxelDrawer::of(Some(&rules), selected)),
            first
        );
    }
}

#[test]
fn posed_voxel_models_honor_the_explicit_draw_style() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let rules = alternate_voxel_style_rules();
    let plain = crate::render::unit_atlas::UnitModel::load(&assets, "PLAIN", Some(&rules)).unwrap();
    let vpl = assets
        .get_ref("VOXELS.VPL")
        .and_then(|data| crate::assets::vpl_file::VplFile::from_bytes(data).ok());
    let key = UnitSpriteKey {
        type_id: "BUFFERED".into(),
        turret_index: 0,
        facing: 8,
        layer: VxlLayer::Body,
        frame: 1,
        slope_type: 0,
        barrel_pitch: 0,
    };
    let tilt = crate::render::unit_atlas::PoseTilt::Ground(crate::render::vxl_raster::GroundPose {
        angles: [0.08, -0.04],
        tunnel_tilt: None,
        body_facing: 8,
        slope_blend: None,
        turret_offset: 0,
    });
    let pixels = |style| {
        plain
            .render_pose(&key, vpl.as_ref(), tilt, style, None)
            .unwrap()
            .0
            .palette_indices
    };
    assert_ne!(pixels(false), pixels(true));
    let mut cache = VxlPoseFrameCache::default();
    for style in [false, true] {
        let entry = cache
            .render(&assets, Some(&rules), &key, tilt, style)
            .unwrap();
        assert_eq!(cache.texels(entry), pixels(style));
        assert_eq!(
            cache.render(&assets, Some(&rules), &key, tilt, style),
            Some(entry)
        );
    }
}

#[test]
fn a_miners_unloading_type_owns_its_voxel_drawer_flags() {
    let rules = alternate_voxel_style_rules();
    let mut sim = crate::sim::world::Simulation::new();
    let miner = sim
        .spawn_object("MINER", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let deployer = sim
        .spawn_object("PLAIN", "Americans", 12, 10, 0, &rules)
        .unwrap();
    let unloading = sim.intern("BUFFERED");
    for id in [miner, deployer] {
        sim.entity_mut_for_test(id)
            .unwrap()
            .seldom
            .display_type_override
            .set_for_test(Some(unloading));
    }
    let drawer = |sim: &crate::sim::world::Simulation, id| {
        let record = crate::app::presentation::frame::ObjectRecord::of_test_sim(sim, &rules, id);
        let type_ref =
            super::super::helpers::unit_draw_type_ref(&record, sim.interner(), Some(&rules));
        let name = sim.interner().resolve(type_ref);
        (
            name.to_owned(),
            VoxelDrawer::of(Some(&rules), name).use_buffer,
        )
    };
    assert_eq!(drawer(&sim, miner), ("BUFFERED".to_owned(), true));
    assert_eq!(drawer(&sim, deployer), ("PLAIN".to_owned(), false));
    sim.entity_mut_for_test(miner)
        .unwrap()
        .seldom
        .display_type_override
        .set_for_test(None);
    assert_eq!(drawer(&sim, miner), ("MINER".to_owned(), false));
}

/// A Siege Chopper's shadow leaves 0xB43180 set (`0x0070744B`), so the tank
/// image the next miss stores is depth-tested (`0x00706ED0`) and stays so on
/// later hits, after a Siege Chopper part clears the flag (`0x0070724F`).
#[test]
fn a_miss_after_a_use_buffer_shadow_keeps_its_depth_tested_image() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let rules = &battle.rules;
    let key = |facing| UnitSpriteKey {
        type_id: "MTNK".into(),
        turret_index: 0,
        facing,
        layer: VxlLayer::Body,
        frame: 0,
        slope_type: 0,
        barrel_pitch: 0,
    };
    let entry = UnitSpriteEntry {
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        pixel_size: [1.0; 2],
        offset_x: 0.0,
        offset_y: 0.0,
        native_draw_bounds: None,
        page: 0,
    };
    let atlas = UnitAtlas::from_test_entries([(key(0), entry), (key(8), entry)].into());
    let pose_cache = std::cell::RefCell::new(VxlPoseFrameCache::default());
    let depth_tested = DepthTestedImages {
        pose_cache: &pose_cache,
        assets: &assets,
        rules: Some(rules),
    };
    let tank = VoxelDrawer::of(Some(rules), "MTNK");
    let chopper = VoxelDrawer::of(Some(rules), "SCHP");
    assert!(chopper.use_buffer && !tank.use_buffer);
    let draw = |images: &mut VoxelImageCache, facing| {
        let image = VoxelImageKey::new(VoxelCache::Body, key(facing));
        cached_part_entry(
            images,
            &atlas,
            Some(&depth_tested),
            tank,
            Some(image),
            &key(facing),
        )
        .unwrap()
        .1
    };
    let mut images = VoxelImageCache::default();
    images.render_shadow(None, chopper.use_buffer, 0, [40, 40]);
    let leaked = draw(&mut images, 0);
    assert!(matches!(leaked, UnitTextureSource::Styled(_)));
    assert!(
        images
            .render_part(chopper.use_buffer, [40, 40])
            .depth_tested
    );
    assert_eq!(draw(&mut images, 0), leaked);
    assert_eq!(draw(&mut images, 8), UnitTextureSource::Stable(0));
}

/// A Siege Chopper miss leaves its depth: 706ED0 clears 0xB43180 after the
/// part draws (`0x0070724F`), so a smaller ordinary render's clear spares it
/// (`0x00753EC5`). The next chopper miss clears depth over that rect alone
/// and draws its voxels outside it only where nearer, and stores that image
/// (#3794). After a render spanning the first, it draws clean.
#[test]
fn a_chopper_miss_after_a_smaller_render_draws_over_stale_depth() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let rules = &battle.rules;
    let model = crate::render::unit_atlas::UnitModel::load(&assets, "SCHP", Some(rules))
        .expect("retail SCHP voxel");
    let vpl = assets
        .get_ref("VOXELS.VPL")
        .and_then(|data| crate::assets::vpl_file::VplFile::from_bytes(data).ok());
    let key = |frame, facing| UnitSpriteKey {
        type_id: "SCHP".into(),
        turret_index: 0,
        facing,
        layer: VxlLayer::Composite,
        frame,
        slope_type: 0,
        barrel_pitch: 0,
    };
    let (first, second) = (key(1, 0), key(0, 8));
    let entry = |key: &UnitSpriteKey| {
        let (sprite, bounds) = model.render(key, vpl.as_ref(), None, &mut None).unwrap();
        UnitSpriteEntry {
            uv_origin: [0.0; 2],
            uv_size: [1.0; 2],
            pixel_size: [sprite.width as f32, sprite.height as f32],
            offset_x: sprite.offset_x,
            offset_y: sprite.offset_y,
            native_draw_bounds: bounds,
            page: 0,
        }
    };
    let atlas = UnitAtlas::from_test_entries(
        [
            (first.clone(), entry(&first)),
            (second.clone(), entry(&second)),
        ]
        .into(),
    );
    let pose_cache = std::cell::RefCell::new(VxlPoseFrameCache::default());
    let live = DepthTestedImages {
        pose_cache: &pose_cache,
        assets: &assets,
        rules: Some(rules),
    };
    let chopper = VoxelDrawer::of(Some(rules), "SCHP");
    let draw = |images: &mut VoxelImageCache, key: &UnitSpriteKey| {
        let image = VoxelImageKey::new(VoxelCache::Body, key.clone());
        cached_part_entry(images, &atlas, Some(&live), chopper, Some(image), key).unwrap()
    };
    let mut images = VoxelImageCache::default();
    assert_eq!(draw(&mut images, &first).1, UnitTextureSource::Stable(0));
    images.render_part(false, entry(&first).native_size());
    assert_eq!(draw(&mut images, &second).1, UnitTextureSource::Stable(0));

    let mut images = VoxelImageCache::default();
    draw(&mut images, &first);
    images.render_part(false, [20, 20]);
    let stale = draw(&mut images, &second);
    assert!(matches!(stale.1, UnitTextureSource::Styled(_)));
    let opaque = |pixels: &[u8]| pixels.iter().filter(|&&pixel| pixel != 0).count();
    let (clean, _) = model
        .render_depth_tested(&second, vpl.as_ref(), None)
        .unwrap();
    let (kept, all) = (
        opaque(&pose_cache.borrow().texels(stale.0)),
        opaque(&clean.palette_indices),
    );
    assert!(0 < kept && kept < all, "{kept} of {all} pixels");
    assert_eq!(draw(&mut images, &second), stale);
}
