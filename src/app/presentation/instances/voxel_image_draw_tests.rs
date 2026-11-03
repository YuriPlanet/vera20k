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
    images.render_shadow(None, chopper.use_buffer, 0);
    let leaked = draw(&mut images, 0);
    assert!(matches!(leaked, UnitTextureSource::DepthTested(_)));
    assert!(images.render_part(chopper.use_buffer));
    assert_eq!(draw(&mut images, 0), leaked);
    assert_eq!(draw(&mut images, 8), UnitTextureSource::Stable(0));
}
