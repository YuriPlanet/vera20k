// The encoded-span voxel raster (7DF9C0/7DFAE0) and the shadow stencil
// (756860), each store placed where the native cursor puts it in the
// 256-square visibility buffer, then moved to the image's place on a pose
// page. A paint store's depth is its place in native store order, so with a
// greater-than test the last store to a pixel wins, as native's does.

struct Draw {
    // 8.8 X | Y << 16 words. Paint: the section origin and the strides of
    // grid axes 0, 1 and 2. Stencil: the origin and the X and Y column steps.
    origin: u32,
    s0: u32,
    s1: u32,
    s2: u32,
    // Paint: grid axis 2's X and Y steps, which a run's skip multiplies.
    dx: i32,
    dy: i32,
    nx: u32,
    ny: u32,
    flags: u32,
    sequence: u32,
    vpl_rows: u32,
    pad: u32,
    // The crop in the visibility buffer: x, y, width, height.
    crop: vec4i,
    // Where the crop's corner lands on the page, and the page size.
    page: vec4i,
    // The draw's lighting page of each normal, four to a word.
    pages: array<vec4u, 16>,
};

// The raster loop walks X and Y from the far edge (the orientation's
// start-X and start-Y), the column backward from its end offset (the
// orientation's lower half), and the lighting pages select a VPL row.
const MIRROR_X: u32 = 1u;
const MIRROR_Y: u32 = 2u;
const REVERSE: u32 = 4u;
const LIT: u32 = 8u;

@group(0) @binding(0) var<uniform> draw: Draw;
@group(0) @binding(1) var vpl: texture_2d<u32>;
// The section's run skips, in column order.
@group(1) @binding(0) var skips: texture_2d<u32>;

struct Store {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) index: u32,
};

// A packed X/Y word: each half truncated on its own, as 7DF9C0 packs a
// skip's product before adding it, carry and all.
fn pair(x: i32, y: i32) -> u32 {
    return (bitcast<u32>(x) & 0xffffu) | (bitcast<u32>(y) << 16u);
}

fn skip_at(index: u32) -> u32 {
    let width = textureDimensions(skips).x;
    return textureLoad(skips, vec2u(index % width, index / width), 0).r;
}

fn lighting_page(normal: u32) -> u32 {
    let word = draw.pages[normal >> 4u][(normal >> 2u) & 3u];
    return (word >> ((normal & 3u) * 8u)) & 255u;
}

// A store at visibility-buffer `pixel`: on the page inside the crop, off the
// page outside it.
fn place(pixel: vec2i, depth: f32, index: u32) -> Store {
    var out: Store;
    out.index = index;
    let local = pixel - draw.crop.xy;
    if any(local < vec2i(0)) || any(local >= draw.crop.zw) {
        out.position = vec4f(-2.0, -2.0, 0.0, 1.0);
        return out;
    }
    let at = (vec2f(draw.page.xy + local) + 0.5) / vec2f(draw.page.zw);
    out.position = vec4f(at.x * 2.0 - 1.0, 1.0 - at.y * 2.0, depth, 1.0);
    return out;
}

// One voxel: column X, Y, colour and normal bytes; its place up the column,
// the column's voxel count, its run and the run count; where the column's
// skips start.
@vertex
fn vs_paint(@location(0) voxel: vec3u) -> Store {
    let color = (voxel.x >> 16u) & 255u;
    let normal = voxel.x >> 24u;
    let up = voxel.y & 255u;
    let count = (voxel.y >> 8u) & 255u;
    let run = (voxel.y >> 16u) & 255u;
    let runs = voxel.y >> 24u;
    var x = voxel.x & 255u;
    var y = (voxel.x >> 8u) & 255u;
    if (draw.flags & MIRROR_X) != 0u {
        x = draw.nx - 1u - x;
    }
    if (draw.flags & MIRROR_Y) != 0u {
        y = draw.ny - 1u - y;
    }
    var cursor = draw.origin + y * draw.s1 + x * draw.s0;
    // Forward: the skips of this run and those below, then the voxels
    // below. Backward: the skips of the runs above, then the voxels above.
    var step = up;
    var first = 0u;
    var last = run + 1u;
    if (draw.flags & REVERSE) != 0u {
        step = count - 1u - up;
        first = run + 1u;
        last = runs;
    }
    for (var i = first; i < last; i++) {
        let skip = i32(skip_at(voxel.z + i));
        cursor += pair(skip * draw.dx, skip * draw.dy);
    }
    cursor += step * draw.s2;
    var index = color;
    if (draw.flags & LIT) != 0u {
        let row = min(lighting_page(normal), draw.vpl_rows - 1u);
        index = textureLoad(vpl, vec2u(color, row), 0).r;
    }
    let order = draw.sequence + (y * draw.nx + x) * 256u + step + 1u;
    let pixel = vec2i(i32((cursor >> 8u) & 255u), i32(cursor >> 24u));
    return place(pixel, f32(order) / 16777216.0, index);
}

// One marked column, x | y << 8; the second store is the adjacent byte.
@vertex
fn vs_stencil(@builtin(vertex_index) second: u32, @location(0) column: u32) -> Store {
    let x = column & 255u;
    let y = column >> 8u;
    // Each 8.8 word wraps on its own.
    let word_x = (draw.origin + y * draw.s1 + x * draw.s0) & 0xffffu;
    let word_y = ((draw.origin >> 16u) + y * (draw.s1 >> 16u) + x * (draw.s0 >> 16u)) & 0xffffu;
    let address = ((word_x >> 8u) | (word_y & 0xff00u)) + second;
    return place(vec2i(i32(address & 255u), i32(address >> 8u)), 0.0, 1u);
}

// The image's rectangle and the padding after it, cleared before its
// stores: crop.zw is the image size here.
@vertex
fn vs_clear(@builtin(vertex_index) vertex: u32) -> Store {
    let corner = vec2i(i32(vertex & 1u), i32(vertex >> 1u));
    let end = min(draw.page.xy + draw.crop.zw + PADDING, draw.page.zw);
    let at = vec2f(draw.page.xy + corner * (end - draw.page.xy)) / vec2f(draw.page.zw);
    var out: Store;
    out.position = vec4f(at.x * 2.0 - 1.0, 1.0 - at.y * 2.0, 0.0, 1.0);
    out.index = 0u;
    return out;
}

@fragment
fn fs_store(input: Store) -> @location(0) u32 {
    return input.index;
}
