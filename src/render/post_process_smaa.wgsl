// SMAA 1x (Jimenez et al. 2013), ported from TC2's `SMAA.hlsli`.
//
// Two helper passes: luma edge detection writes an RG edge map, then the
// blending-weight pass searches along each edge (using the SearchTex run-length
// table) and looks the coverage up in AreaTex. The presentation pass applies
// the weights in `post_process.wgsl`.
//
// Diagonals are disabled (SMAA's Medium preset): only the horizontal/vertical
// and corner paths are ported, which is the quality tier TC2's SMAA_Medium
// script uses.

struct SmaaParams {
    // (edge threshold, search steps, diagonal steps, unused)
    info: vec4f,
    // (viewport x, viewport y, width, height)
    viewport: vec4f,
    // (corner rounding 0..1, unused, unused, unused)
    flags: vec4f,
};

@group(0) @binding(0) var t_source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> smaa: SmaaParams;
@group(0) @binding(2) var t_area: texture_2d<f32>;
@group(0) @binding(3) var t_search: texture_2d<f32>;
@group(0) @binding(4) var s_linear: sampler;

const AREA_PIXEL = vec2f(1.0 / 160.0, 1.0 / 560.0);

fn gamma_of(c: vec3f) -> vec3f {
    return pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.2));
}

fn inside_viewport(p: vec2i) -> bool {
    let v = smaa.viewport;
    let f = vec2f(p);
    return !(f.x < v.x || f.y < v.y || f.x >= v.x + v.z || f.y >= v.y + v.w);
}

fn texel_luma(p: vec2i) -> f32 {
    let dims = vec2i(textureDimensions(t_source));
    let c = gamma_of(textureLoad(t_source, clamp(p, vec2i(0), dims - vec2i(1)), 0).rgb);
    return dot(c, vec3f(0.2126, 0.7152, 0.0722));
}

fn sample_edges(uv: vec2f) -> vec2f {
    return textureSampleLevel(t_source, s_linear, uv, 0.0).rg;
}

fn sample_edges_off(uv: vec2f, offset: vec2i) -> vec2f {
    let texel = 1.0 / vec2f(textureDimensions(t_source));
    return textureSampleLevel(t_source, s_linear, uv + vec2f(offset) * texel, 0.0).rg;
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

/// Pass 1: luma edge detection. R = edge between this texel and its left
/// neighbour, G = edge between this texel and the one above.
@fragment
fn fs_edges(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let p = vec2i(position.xy);
    if (!inside_viewport(p)) {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    let threshold = max(smaa.info.x, 0.0);
    let l = texel_luma(p);
    let left = texel_luma(p + vec2i(-1, 0));
    let top = texel_luma(p + vec2i(0, -1));
    var delta = vec2f(abs(l - left), abs(l - top));
    var edges = step(vec2f(threshold), delta);
    if (edges.x + edges.y == 0.0) {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    let right = texel_luma(p + vec2i(1, 0));
    let bottom = texel_luma(p + vec2i(0, 1));
    var max_delta = max(delta, vec2f(abs(l - right), abs(l - bottom)));
    let left_left = texel_luma(p + vec2i(-2, 0));
    let top_top = texel_luma(p + vec2i(0, -2));
    max_delta = max(max_delta, vec2f(abs(left - left_left), abs(top - top_top)));
    let final_delta = max(max_delta.x, max_delta.y);
    // Local contrast adaptation: a small delta beside a much larger one is
    // kept out, so gradients do not turn into edges.
    edges *= step(vec2f(final_delta), 2.0 * delta);
    return vec4f(edges, 0.0, 1.0);
}

/// The SearchTex run-length table, shared by all four search directions.
fn search_length(e: vec2f, offset: f32) -> f32 {
    var scale = vec2f(66.0, 33.0) * vec2f(0.5, -1.0);
    var bias = vec2f(66.0, 33.0) * vec2f(offset, 1.0);
    scale += vec2f(-1.0, 1.0);
    bias += vec2f(0.5, -0.5);
    scale *= 1.0 / vec2f(64.0, 16.0);
    bias *= 1.0 / vec2f(64.0, 16.0);
    return textureSampleLevel(t_search, s_linear, scale * e + bias, 0.0).r;
}

fn search_x_left(uv: vec2f, end: f32) -> f32 {
    let texel = 1.0 / vec2f(textureDimensions(t_source));
    var coord = uv;
    var e = vec2f(0.0, 1.0);
    loop {
        if (!(coord.x > end && e.y > 0.8281 && e.x == 0.0)) {
            break;
        }
        e = sample_edges(coord);
        coord.x -= 2.0 * texel.x;
    }
    let offset = 3.25 - (255.0 / 127.0) * search_length(e, 0.0);
    return coord.x + texel.x * offset;
}

fn search_x_right(uv: vec2f, end: f32) -> f32 {
    let texel = 1.0 / vec2f(textureDimensions(t_source));
    var coord = uv;
    var e = vec2f(0.0, 1.0);
    loop {
        if (!(coord.x < end && e.y > 0.8281 && e.x == 0.0)) {
            break;
        }
        e = sample_edges(coord);
        coord.x += 2.0 * texel.x;
    }
    let offset = 3.25 - (255.0 / 127.0) * search_length(e, 0.5);
    return coord.x - texel.x * offset;
}

fn search_y_up(uv: vec2f, end: f32) -> f32 {
    let texel = 1.0 / vec2f(textureDimensions(t_source));
    var coord = uv;
    var e = vec2f(1.0, 0.0);
    loop {
        if (!(coord.y > end && e.x > 0.8281 && e.y == 0.0)) {
            break;
        }
        e = sample_edges(coord);
        coord.y -= 2.0 * texel.y;
    }
    let offset = 3.25 - (255.0 / 127.0) * search_length(e.yx, 0.0);
    return coord.y + texel.y * offset;
}

fn search_y_down(uv: vec2f, end: f32) -> f32 {
    let texel = 1.0 / vec2f(textureDimensions(t_source));
    var coord = uv;
    var e = vec2f(1.0, 0.0);
    loop {
        if (!(coord.y < end && e.x > 0.8281 && e.y == 0.0)) {
            break;
        }
        e = sample_edges(coord);
        coord.y += 2.0 * texel.y;
    }
    let offset = 3.25 - (255.0 / 127.0) * search_length(e.yx, 0.5);
    return coord.y - texel.y * offset;
}

/// Trilinear AreaTex lookup; the table is compressed quadratically, so the
/// distances arrive already square-rooted.
fn area(dist: vec2f, e1: f32, e2: f32) -> vec2f {
    var coord = vec2f(16.0, 16.0) * round(4.0 * vec2f(e1, e2)) + dist;
    coord = AREA_PIXEL * coord + 0.5 * AREA_PIXEL;
    return textureSampleLevel(t_area, s_linear, coord, 0.0).rg;
}

fn horizontal_corner(w: vec2f, left_uv: vec2f, right_uv: vec2f, d: vec2f) -> vec2f {
    let left_right = step(d, d.yx);
    var rounding = (1.0 - smaa.flags.x) * left_right;
    rounding /= left_right.x + left_right.y;
    var factor = vec2f(1.0, 1.0);
    factor.x -= rounding.x * sample_edges_off(left_uv, vec2i(0, 1)).r;
    factor.x -= rounding.y * sample_edges_off(right_uv, vec2i(1, 1)).r;
    factor.y -= rounding.x * sample_edges_off(left_uv, vec2i(0, -2)).r;
    factor.y -= rounding.y * sample_edges_off(right_uv, vec2i(1, -2)).r;
    return w * clamp(factor, vec2f(0.0), vec2f(1.0));
}

fn vertical_corner(w: vec2f, top_uv: vec2f, bottom_uv: vec2f, d: vec2f) -> vec2f {
    let left_right = step(d, d.yx);
    var rounding = (1.0 - smaa.flags.x) * left_right;
    rounding /= left_right.x + left_right.y;
    var factor = vec2f(1.0, 1.0);
    factor.x -= rounding.x * sample_edges_off(top_uv, vec2i(1, 0)).g;
    factor.x -= rounding.y * sample_edges_off(bottom_uv, vec2i(1, 1)).g;
    factor.y -= rounding.x * sample_edges_off(top_uv, vec2i(-2, 0)).g;
    factor.y -= rounding.y * sample_edges_off(bottom_uv, vec2i(-2, 1)).g;
    return w * clamp(factor, vec2f(0.0), vec2f(1.0));
}

/// Pass 2: blending weights. R/G = left/right lateral weights, B/A = top/bottom
/// vertical weights, matching SMAA's convention.
@fragment
fn fs_weights(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let p = vec2i(position.xy);
    if (!inside_viewport(p)) {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    let dims = vec2f(textureDimensions(t_source));
    let texel = 1.0 / dims;
    let uv = position.xy * texel;
    let pixcoord = position.xy;
    let steps = max(smaa.info.y, 1.0);

    var weights = vec4f(0.0);
    let e = sample_edges(uv);

    // Edge above this texel: search left and right along the row.
    if (e.y > 0.0) {
        let left_start = uv + vec2f(-0.25, -0.125) * texel;
        let right_start = uv + vec2f(1.25, -0.125) * texel;
        let cross_y = uv.y + (-0.25) * texel.y;
        let left_x = search_x_left(left_start, left_start.x - 2.0 * steps * texel.x);
        let right_x = search_x_right(right_start, right_start.x + 2.0 * steps * texel.x);
        let left_uv = vec2f(left_x, cross_y);
        let right_uv = vec2f(right_x, cross_y);
        let e1 = sample_edges(left_uv).r;
        let e2 = sample_edges_off(right_uv, vec2i(1, 0)).r;
        var d = abs(round(vec2f(dims.x) * vec2f(left_x, right_x) - pixcoord.x));
        let lateral = area(sqrt(d), e1, e2);
        let fixed = horizontal_corner(lateral, vec2f(left_x, uv.y), vec2f(right_x, uv.y), d);
        weights.x = fixed.x;
        weights.y = fixed.y;
    }

    // Edge left of this texel: search up and down along the column.
    if (e.x > 0.0) {
        let up_start = uv + vec2f(-0.125, -0.25) * texel;
        let down_start = uv + vec2f(-0.125, 1.25) * texel;
        let cross_x = uv.x + (-0.25) * texel.x;
        let top_y = search_y_up(up_start, up_start.y - 2.0 * steps * texel.y);
        let bottom_y = search_y_down(down_start, down_start.y + 2.0 * steps * texel.y);
        let top_uv = vec2f(cross_x, top_y);
        let bottom_uv = vec2f(cross_x, bottom_y);
        let e1 = sample_edges(top_uv).g;
        let e2 = sample_edges_off(bottom_uv, vec2i(0, 1)).g;
        var d = abs(round(vec2f(dims.y) * vec2f(top_y, bottom_y) - pixcoord.y));
        let vertical = area(sqrt(d), e1, e2);
        let fixed = vertical_corner(vertical, vec2f(uv.x, top_y), vec2f(uv.x, bottom_y), d);
        weights.z = fixed.x;
        weights.w = fixed.y;
    }

    return weights;
}
