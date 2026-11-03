use super::*;
use crate::render::unit_atlas::VxlLayer;

fn body(type_id: &str, facing: u8) -> VoxelImageKey {
    VoxelImageKey::new(
        VoxelCache::Body,
        UnitSpriteKey {
            type_id: type_id.into(),
            turret_index: 0,
            facing,
            layer: VxlLayer::Body,
            frame: 0,
            slope_type: 0,
            barrel_pitch: 0,
        },
    )
}

/// 706ED0 sets and clears 0xB43180 around a UseBuffer part (0x00706F12,
/// 0x0070724F); 707280 leaves it set after a UseBuffer shadow (0x0070744B).
#[test]
fn a_use_buffer_shadow_leaves_later_misses_depth_tested() {
    let mut cache = VoxelImageCache::default();
    let ordinary = |cache: &mut VoxelImageCache, key| cache.draw(key, false, 0, |_| 100);
    assert!(cache.draw(body("SCHP", 0), true, 0, |_| 100).depth_tested);
    assert!(!ordinary(&mut cache, body("MTNK", 0)).depth_tested);
    let shadow = VoxelImageKey::new(VoxelCache::Shadow, body("SCHP", 0).sprite);
    assert!(!cache.shadow_hit(&shadow));
    cache.render_shadow(Some(shadow.clone()), true, 100);
    assert!(cache.shadow_hit(&shadow));
    // A miss stores the leaked style; a hit keeps the first one.
    assert!(ordinary(&mut cache, body("MTNK", 8)).depth_tested);
    assert!(!ordinary(&mut cache, body("MTNK", 0)).depth_tested);
    assert!(cache.render_part(false));
    // The next UseBuffer part render clears it again.
    assert!(cache.draw(body("SCHP", 8), true, 0, |_| 100).depth_tested);
    assert!(!ordinary(&mut cache, body("MTNK", 16)).depth_tested);
}

/// 706AD5..706B0A: a store that does not fit frees every type's caches
/// (0x005F99E0) and stores again, so a gun key takes the pitch drawn then.
#[test]
fn a_store_that_does_not_fit_flushes_every_cache() {
    let mut cache = VoxelImageCache::default();
    let mut barrel = body("HTNK", 0);
    barrel.cache = VoxelCache::Barrel;
    assert_eq!(
        cache
            .draw(barrel.clone(), false, 2, |_| POOL_BYTES - 10)
            .barrel_pitch,
        2
    );
    assert_eq!(
        cache.draw(body("MTNK", 0), false, 0, |_| 10),
        cache.draw(body("MTNK", 0), false, 0, |_| 0)
    );
    assert_eq!(cache.flushes(), 0, "a store filling the pool exactly fits");
    assert_eq!(cache.draw(barrel.clone(), false, 5, |_| 1).barrel_pitch, 2);
    cache.draw(body("MTNK", 8), false, 0, |_| 1);
    assert_eq!(cache.flushes(), 1);
    assert_eq!(cache.draw(barrel, false, 5, |_| 1).barrel_pitch, 5);
}

/// 6C89E0 stores a 12-byte header and each row as 661590's 2-byte length
/// plus 661530's runs: a byte per nonzero pixel, two per 255 zeros.
#[test]
fn pool_bytes_follow_the_zero_run_encoding() {
    let mut pixels = vec![0u8; 2 * 300];
    pixels[300] = 7;
    pixels[303] = 9;
    // Row 0: 256 zeros = two runs. Row 1: 7, two zeros, 9, then 252 zeros.
    assert_eq!(
        pool_bytes(&pixels, 300, [0, 0, 256, 2]),
        12 + (2 + 4) + (2 + 1 + 2 + 1 + 2)
    );
    assert_eq!(pool_bytes(&pixels, 300, [1, 1, 2, 1]), 12 + 2 + 2);
}
