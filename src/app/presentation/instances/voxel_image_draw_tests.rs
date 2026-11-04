use super::*;
use crate::render::unit_atlas::UnitAtlas;
use crate::render::unit_pose_cache::VxlPoseFrameCache;
use crate::render::voxel_image_cache::{VoxelCache, VoxelImageCache, VoxelImageKey};

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
    assert!(matches!(leaked, UnitTextureSource::DepthTested(_)));
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
    assert!(matches!(stale.1, UnitTextureSource::DepthTested(_)));
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
