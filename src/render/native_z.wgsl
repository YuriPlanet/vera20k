// Native stored Z is an unsigned 16-bit word; lower is nearer, empty=65535.
// The independent row seed is32768. Preserve the entire word in Depth32Float.
// TREE leaves 4990E0/497390 compare signed candidate BEFORE this low16 store.
// The depth test compares stored words instead; zsprite_shader.wgsl's entry
// points say how a candidate outside 0..=65535 is admitted.
fn stored_native_depth(z: i32) -> f32 {
    return f32(bitcast<u32>(z) & 65535u) / 65535.0;
}

fn decoded_native_z(depth: f32) -> i32 {
    return i32(round(depth * 65535.0));
}

// The world row whose Z is the seed's 32768: the view's top row `view_top`
// (camera_pos.y + native_z_origin_y, the Z buffer's row 0) less the rows the
// seed has panned (`native_z::ZSeed`). Z rows count from it; clips keep the
// view's own top.
fn native_z_origin_row(view_top: f32, pan: f32) -> i32 {
    return i32(round(view_top)) - i32(pan);
}

// The view's world rows [top, bottom): `view_top` as above, `view_height`
// the tactical view's height in world pixels (fx_params.w; 0 takes
// `target_height`). CameraUniform::view_rows.
fn view_rows(view_top: f32, view_height: f32, target_height: f32) -> vec2i {
    let top = i32(round(view_top));
    return vec2i(top, top + i32(round(select(target_height, view_height, view_height > 0.0))));
}

// The world rows [top, bottom) an SHP blit over `rows` seeds its Z walk from
// in the view over `view`: Bit_Blit clips a raw frame (gradient bit 0x4000)
// at both ends, the RLE walker a top-seeded entry at the top alone
// (native_z::shp_seed_rows).
fn shp_seed_rows(rows: vec2i, z_gradient: u32, view: vec2i) -> vec2i {
    let raw = (z_gradient & 0x4000u) != 0u;
    var seed = rows;
    if (raw || (z_gradient & 0xFFu) != 2u) {
        seed.x = max(seed.x, view.x);
    }
    if (raw) {
        seed.y = min(seed.y, view.y);
    }
    return seed;
}

// Compatibility sprite depth remains a world/sort scalar on the CPU. Only
// depth-tested batch entrypoints adapt it, including legacy UI depth policies.
fn compatibility_depth_on_native_axis(depth: f32, z_origin_row: i32,
                                     origin_y: f32, world_height: f32) -> f32 {
    let world_row = origin_y + (1.0 - depth) * max(world_height, 1.0);
    // Preserve legacy fractional epsilon ordering. These compatibility inputs
    // already lost rows at CPU clamps; only native per-pixel producers claim
    // exact u16 values. TREE interprets a mixed old value via decoded_native_z.
    return clamp((32768.0 + f32(z_origin_row) - world_row) / 65535.0, 0.0, 1.0);
}

fn world_row_from_native_depth(depth: f32, z_origin_row: i32) -> f32 {
    return f32(32768 - decoded_native_z(depth) + z_origin_row);
}

// Native Z of the row `row` (0 = top) of a blit whose top is at screen row
// `screen_top` (Z-buffer coordinates, YOrigin = 0, counted from
// native_z_origin_row), walked with gradient `entry` (0, 1, 2) and Z term
// `z_adjust`. Mirrors `native_z::sprite_row_z`.
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

// The Unit final composite (`0x0073B140`, Unit vtable +0x55C) that a voxel
// body and a turreted SHP Unit's body and turret (`0x0073C7AC..0x0073CDF9`)
// reach the screen through: every part shares one rectangle
// (`rect_top_height`, world top and height), so their depth decision is
// identical at every pixel. With 0x400 the rectangle splits into a
// full-width upper h-16 and a bottom 16, seeded independently by
// Standard_SHP_blitter. Each blit is a Bit_Blit, which clips its rectangle
// to the view rows `view` before seeding (0x00437461, then
// 0x004374BB..0x004375A7); its bottom-seeded walk follows the clipped
// bottom. Returns the candidate in x, and y = 0 where the clip cuts the
// fragment. Mirrors `native_z::voxel_depth_region`.
fn unit_composite_candidate(rect_top_height: vec2f, world_y: f32, z_gradient: u32,
                            z_adjust_in: f32, view: vec2i, z_origin: i32) -> vec2i {
    var rect_top: f32 = rect_top_height.x;
    var height: i32 = max(i32(round(rect_top_height.y)), 1);
    var gradient: u32 = z_gradient & 0xFFu;
    var z_adjust: i32 = i32(round(z_adjust_in));
    // Retain full quads/UVs so both regions sample the same source pixel grid.
    if ((z_gradient & 0x400u) != 0u) {
        rect_top = round(rect_top);
        if (height > 16) {
            let boundary: f32 = rect_top + f32(height - 16);
            if (world_y < boundary) {
                height = height - 16;
                gradient = 0u;
                z_adjust = z_adjust - 5;
            } else {
                rect_top = boundary;
                height = 16;
                gradient = 2u;
            }
        }
    }
    if ((z_gradient & 0x1400u) != 0u) {
        rect_top = round(rect_top);
        let region_top: f32 = rect_top;
        let region_bottom: f32 = rect_top + f32(height);
        let bottom: f32 = min(region_bottom, f32(view.y));
        rect_top = max(rect_top, f32(view.x));
        height = i32(bottom - rect_top);
        // Only actual tactical clip intersections cut coverage; the full quad
        // and the row clamp own natural edges. Re-testing those edges
        // against interpolated coordinates can discard a boundary source row.
        if (height <= 0
            || (rect_top > region_top && world_y < rect_top)
            || (bottom < region_bottom && world_y >= bottom)) {
            return vec2i(0, 0);
        }
    }
    let screen_top: i32 = i32(round(rect_top)) - z_origin;
    let row: i32 = clamp(i32(floor(world_y - rect_top)), 0, height - 1);
    return vec2i(native_row_z(gradient, screen_top, height, z_adjust, row), 1);
}
