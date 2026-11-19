// Native stored Z is an unsigned 16-bit word; lower is nearer, empty=65535.
// The independent row seed is32768. Preserve the entire word in Depth32Float.
// TREE leaves 4990E0/497390 compare signed candidate BEFORE this low16 store.
// RESIDUAL: the sprite and voxel pipelines compare this low16 word, where
// every Z leaf compares the signed 32-bit candidate (draw iff cand < stored).
// They differ only for cand < 0 or > 65535, about 32K rows from the seed.
fn stored_native_depth(z: i32) -> f32 {
    return f32(bitcast<u32>(z) & 65535u) / 65535.0;
}

fn decoded_native_z(depth: f32) -> i32 {
    return i32(round(depth * 65535.0));
}

// Compatibility sprite depth remains a world/sort scalar on the CPU. Only
// depth-tested batch entrypoints adapt it, including legacy UI depth policies.
fn compatibility_depth_on_native_axis(depth: f32, camera_y: f32,
                                     origin_y: f32, world_height: f32) -> f32 {
    let world_row = origin_y + (1.0 - depth) * max(world_height, 1.0);
    // Preserve legacy fractional epsilon ordering. These compatibility inputs
    // already lost rows at CPU clamps; only native per-pixel producers claim
    // exact u16 values. TREE interprets a mixed old value via decoded_native_z.
    return clamp((32768.0 + round(camera_y) - world_row) / 65535.0, 0.0, 1.0);
}

fn world_row_from_native_depth(depth: f32, camera_y: f32) -> f32 {
    return f32(32768 - decoded_native_z(depth)) + round(camera_y);
}

// Native Z of the row `row` (0 = top) of a blit whose top is at screen row
// `screen_top` (Z-buffer coordinates, YOrigin = 0), walked with gradient
// `entry` (0, 1, 2) and Z term `z_adjust`. Mirrors `native_z::sprite_row_z`.
fn native_row_z(entry: u32, screen_top: i32, height: i32, z_adjust: i32, row: i32) -> i32 {
    let default_z: i32 = 32768;
    var seed: i32;
    var accum: i32 = 0;
    var increment: i32;
    var threshold: i32;
    var step_dir: i32;
    if (entry == 2u) {
        increment = 1;
        threshold = 3;
        step_dir = 1;
        let raw: i32 = ((default_z - height - screen_top + 1) & 0xFFFF) + z_adjust;
        seed = (raw / 3) * 3 - height / 3;
        accum = 3 - (height % 3);
        if (accum == 3) {
            accum = 0;
            seed = seed + 1;
        }
    } else if (entry == 1u) {
        increment = 2;
        threshold = 3;
        step_dir = -1;
        let raw: i32 = ((default_z - screen_top) & 0xFFFF) + z_adjust;
        seed = (raw / 3) * 3;
    } else {
        increment = 1;
        threshold = 1;
        step_dir = -1;
        seed = ((default_z - screen_top) & 0xFFFF) + z_adjust;
    }
    let steps: i32 = (accum + max(row, 0) * increment) / threshold;
    return seed + step_dir * steps;
}
