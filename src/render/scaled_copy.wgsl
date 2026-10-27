// A scaled copy of a surface: the match zoom and the sidebar's size.
//
// Source pixel (0, 0) lands at `origin`, and each source pixel covers
// `scale` x `scale` target pixels. A whole scale copies blocks. Between whole
// scales a target pixel can straddle two source pixels on an axis; it blends
// them by how much of it each covers (a box filter), so a source pixel keeps
// its own colour wherever it covers a target pixel alone and only the seams
// soften. When shrinking, integrate every source texel covered by the target
// pixel instead of dropping thin details through nearest-neighbor sampling.
// The draw's scissor covers exactly the scaled surface.

struct Params {
    origin: vec2f,   // target pixel where source pixel (0, 0) lands
    scale: f32,      // target pixels per source pixel, positive
    sharp: f32,      // 1 for a whole scale
};

@group(0) @binding(0) var t_source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
    // One triangle that covers the whole target.
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn texel(at: vec2i) -> vec4f {
    let last = vec2i(textureDimensions(t_source)) - 1;
    return textureLoad(t_source, clamp(at, vec2i(0), last), 0);
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    // `position.xy` is the pixel centre; here it is in source pixels.
    let centre = (position.xy - params.origin) / params.scale;
    if params.sharp != 0.0 {
        // The floor names the block.
        return texel(vec2i(floor(centre)));
    }
    if params.scale < 1.0 {
        // Area resampling of the completed image: palette, transparent holes,
        // depth and destination-dependent blits have already been resolved.
        // textureLoad addresses texels directly:
        // https://www.w3.org/TR/WGSL/#textureload
        let lo = (position.xy - vec2f(0.5) - params.origin) / params.scale;
        let hi = (position.xy + vec2f(0.5) - params.origin) / params.scale;
        var sum = vec4f(0.0);
        for (var y = i32(floor(lo.y)); y < i32(ceil(hi.y)); y += 1) {
            let wy = min(hi.y, f32(y + 1)) - max(lo.y, f32(y));
            for (var x = i32(floor(lo.x)); x < i32(ceil(hi.x)); x += 1) {
                let wx = min(hi.x, f32(x + 1)) - max(lo.x, f32(x));
                sum += texel(vec2i(x, y)) * (wx * wy);
            }
        }
        return sum * (params.scale * params.scale);
    }
    // The target pixel spans 1 / scale source pixels, so at most two per
    // axis: `first` holds its top-left corner, `t` is its share past the seam.
    let half = 0.5 / params.scale;
    let first = floor(centre - half);
    let t = clamp((centre + half - (first + 1.0)) * params.scale, vec2f(0.0), vec2f(1.0));
    let at = vec2i(first);
    let top = mix(texel(at), texel(at + vec2i(1, 0)), t.x);
    let bottom = mix(texel(at + vec2i(0, 1)), texel(at + vec2i(1, 1)), t.x);
    return mix(top, bottom, t.y);
}
