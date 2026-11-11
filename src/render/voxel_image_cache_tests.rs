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
    let ordinary =
        |cache: &mut VoxelImageCache, key| cache.draw(key, false, 0, [40, 40], |_, _| (None, 100));
    assert!(
        cache
            .draw(body("SCHP", 0), true, 0, [40, 40], |_, _| (None, 100))
            .depth_tested
    );
    assert!(!ordinary(&mut cache, body("MTNK", 0)).depth_tested);
    let shadow = VoxelImageKey::new(VoxelCache::Shadow, body("SCHP", 0).sprite);
    assert!(!cache.shadow_hit(&shadow));
    cache.render_shadow(Some(shadow.clone()), true, 100, [40, 40]);
    assert!(cache.shadow_hit(&shadow));
    // A miss stores the leaked style; a hit keeps the first one.
    assert!(ordinary(&mut cache, body("MTNK", 8)).depth_tested);
    assert!(!ordinary(&mut cache, body("MTNK", 0)).depth_tested);
    assert!(cache.render_part(false, [40, 40]).depth_tested);
    // The next UseBuffer part render clears it again.
    assert!(
        cache
            .draw(body("SCHP", 8), true, 0, [40, 40], |_, _| (None, 100))
            .depth_tested
    );
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
            .draw(barrel.clone(), false, 2, [40, 40], |_, _| (
                None,
                POOL_BYTES - 10
            ))
            .barrel_pitch,
        2
    );
    assert_eq!(
        cache.draw(body("MTNK", 0), false, 0, [40, 40], |_, _| (None, 10)),
        cache.draw(body("MTNK", 0), false, 0, [40, 40], |_, _| (None, 0))
    );
    assert_eq!(cache.flushes(), 0, "a store filling the pool exactly fits");
    assert_eq!(
        cache
            .draw(barrel.clone(), false, 5, [40, 40], |_, _| (None, 1))
            .barrel_pitch,
        2
    );
    cache.draw(body("MTNK", 8), false, 0, [40, 40], |_, _| (None, 1));
    assert_eq!(cache.flushes(), 1);
    assert_eq!(
        cache
            .draw(barrel, false, 5, [40, 40], |_, _| (None, 1))
            .barrel_pitch,
        5
    );
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

/// A new scenario frees only the caches its rules read reloads (0x0071609C ->
/// 0x005F8BF0); 0xB43180 (0x0052BE04, once a process) and the pool cursor
/// (0x005F99E0) carry over, and so does a building gun's stored image.
#[test]
fn a_new_scenario_keeps_the_flag_the_pool_and_unreloaded_types() {
    let mut cache = VoxelImageCache::default();
    let mut gun = body("GTGCAN", 0);
    gun.cache = VoxelCache::Turret;
    cache.draw(gun.clone(), false, 0, [40, 40], |_, _| {
        (None, POOL_BYTES - 10)
    });
    cache.draw(body("MTNK", 0), false, 0, [40, 40], |_, _| (None, 10));
    cache.render_shadow(None, true, 0, [40, 40]);
    cache.start_scenario(|type_id| type_id == "MTNK");
    assert!(cache.hit(&gun).is_some());
    assert!(cache.hit(&body("MTNK", 0)).is_none());
    assert!(
        cache
            .draw(body("MTNK", 0), false, 0, [40, 40], |_, _| (None, 10))
            .depth_tested
    );
    assert_eq!(
        cache.flushes(),
        1,
        "the dropped image still holds its bytes"
    );
}

/// 753E00 clears depth over the previous render's rect, rows and columns
/// inclusive, only while 0xB43180 is set (`0x00753EC5`). A Siege Chopper
/// part clears the flag after it draws (`0x0070724F`), so the next one keeps
/// its depth outside a smaller ordinary render's rect; a shadow sets the flag
/// before its clear (`0x007072FC`).
#[test]
fn a_use_buffer_render_keeps_depth_outside_the_previous_rect() {
    // A 20-pixel image's rect spans (118, 118) to (138, 138).
    for (column, stale) in [(138, false), (139, true)] {
        let mut cache = VoxelImageCache::default();
        assert!(cache.render_part(true, [40, 40]).live);
        cache.depth_map()[138 * 256 + column] = 9;
        assert!(!cache.render_part(false, [20, 20]).depth_tested);
        assert_eq!(cache.render_part(true, [40, 40]).stale, stale);
    }
    let mut cache = VoxelImageCache::default();
    cache.render_part(true, [40, 40]);
    cache.depth_map()[139 * 256 + 139] = 9;
    cache.render_shadow(None, true, 0, [40, 40]);
    assert_eq!(
        cache.render_part(false, [20, 20]),
        PartRender {
            depth_tested: true,
            live: false,
            stale: false,
        }
    );
}
