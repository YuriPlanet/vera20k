// VERA's HD art terrain, VERA-only: no gamemd counterpart. Draws the 2x
// upscaled theater tiles over the wheel-zoomed frame, where the native world
// still shows that tile's own pixel.
//
// As `hd_art.wgsl`: the world is composed at native pixels first, exactly as
// before; this pass only replaces pixels of the zoomed copy. For each frame
// pixel it finds the tile texel under it in the tile atlas:
//   * transparent (outside the tile's shape): untouched;
//   * the native composed colour no longer matches the tile's own colour
//     under a plain light (a smudge, an overlay, a unit, a later tile or a
//     fog edge drew over it): untouched;
//   * otherwise the HD tile's pixel, multiplied by the light the native pixel
//     received (cell lighting, shadows), so the ground keeps its light.
// The tiles are RGB in their theater palette already, so no palette moves.
//
// `srgb_encode` / `srgb_decode` are prepended by hd_terrain.rs.

struct Params {
    // Frame width, height, frame pixels per native pixel, debug view.
    frame: vec4f,
    // x: 1 when the atlas is an sRGB texture (its loads are linear already).
    mode: vec4f,
};

@group(0) @binding(0) var composition: texture_2d<f32>;
@group(0) @binding(1) var tiles: texture_2d<f32>;
@group(0) @binding(2) var hd_tiles: texture_2d<f32>;
@group(0) @binding(3) var<uniform> params: Params;

struct Instance {
    // Native composition pixel of the tile's top-left corner.
    @location(0) origin: vec2f,
    // Native tile size in pixels.
    @location(1) size: vec2f,
    // The tile's top-left in the tile atlas.
    @location(2) atlas_origin: vec2u,
    // The tile's top-left in the HD page (2x pixels).
    @location(3) hd_origin: vec2u,
};

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) origin: vec2f,
    @location(1) @interpolate(flat) size: vec2f,
    @location(2) @interpolate(flat) atlas_origin: vec2u,
    @location(3) @interpolate(flat) hd_origin: vec2u,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32, instance: Instance) -> VertexOutput {
    let corner = vec2f(f32(index & 1u), f32((index >> 1u) & 1u));
    let frame_px = (instance.origin + corner * instance.size) * params.frame.z;
    var out: VertexOutput;
    out.position = vec4f(
        frame_px.x / params.frame.x * 2.0 - 1.0,
        1.0 - frame_px.y / params.frame.y * 2.0,
        0.0,
        1.0,
    );
    out.origin = instance.origin;
    out.size = instance.size;
    out.atlas_origin = instance.atlas_origin;
    out.hd_origin = instance.hd_origin;
    return out;
}

fn luma(rgb: vec3f) -> f32 {
    return dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
}

// The light the native pixel at `local` (tile pixels) received, and 1 in
// w, when it still shows this tile's own colour under a plain light; 0 in
// w otherwise (outside the tile, transparent, or drawn over). Per channel so
// tinted cell light stays, but held near its brightness: a dark channel of
// the tile must not turn noise into a colour cast.
fn pixel_light(input: VertexOutput, local: vec2i) -> vec4f {
    if any(local < vec2i(0)) || any(vec2f(local) >= input.size) {
        return vec4f(0.0);
    }
    let texel = textureLoad(tiles, input.atlas_origin + vec2u(local), 0);
    if texel.a < 0.5 {
        return vec4f(0.0);
    }
    let native = vec2i(input.origin) + local;
    let composed = textureLoad(composition, vec2u(native), 0).rgb;
    let base = select(srgb_decode(texel.rgb), texel.rgb, params.mode.x == 1.0);
    let base_l = luma(base);
    let composed_l = luma(composed);
    if base_l < 0.004 {
        if composed_l > 0.02 {
            return vec4f(0.0);
        }
    } else {
        let k = composed_l / base_l;
        let error = length(composed - base * k) / max(composed_l, 0.02);
        if k < 0.15 || k > 4.0 || error > 0.2 {
            return vec4f(0.0);
        }
    }
    let k = (composed_l + 0.01) / (base_l + 0.01);
    let light = clamp((composed + 0.01) / (base + 0.01), vec3f(k * 0.8), vec3f(k * 1.25));
    return vec4f(clamp(light, vec3f(0.0), vec3f(4.0)), 1.0);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4f {
    let scale = params.frame.z;
    let at = input.position.xy / scale - input.origin;
    let local = vec2i(floor(at));
    let own = pixel_light(input, local);
    if own.w == 0.0 {
        discard;
    }

    // The HD pixel: two per native pixel, inside the tile. From 200% each
    // frame pixel takes the nearest; below it the four around its place
    // blend (at 100% exactly the native pixel's own four), skipping clear
    // ones, so the art is resampled instead of thinned.
    let fine = clamp(at * 2.0, vec2f(0.0), input.size * 2.0 - 1.0);
    var hd = textureLoad(hd_tiles, input.hd_origin + vec2u(fine), 0);
    if scale < 2.0 {
        let last = vec2i(input.size * 2.0) - 1;
        let p = at * 2.0 - 0.5;
        let c = vec2i(floor(p));
        let w = p - floor(p);
        var sum = vec4f(0.0);
        for (var i = 0; i < 4; i += 1) {
            let o = vec2i(i & 1, i >> 1u);
            let q = clamp(c + o, vec2i(0), last);
            let t = textureLoad(hd_tiles, input.hd_origin + vec2u(q), 0);
            let wx = select(1.0 - w.x, w.x, o.x == 1);
            let wy = select(1.0 - w.y, w.y, o.y == 1);
            let weight = wx * wy * step(0.5, t.a);
            sum += vec4f(srgb_decode(t.rgb) * weight, weight);
        }
        if sum.w > 0.001 {
            hd = vec4f(srgb_encode(sum.rgb / sum.w), 1.0);
        }
    }
    if hd.a < 0.5 {
        discard;
    }

    // The light between native pixel centres, from this tile's neighbouring
    // pixels that still show it: shadow edges and light steps on the ground
    // fall between the HD pixels instead of in native blocks. A neighbour
    // that is not this tile's keeps this pixel's own light.
    let t = at - 0.5;
    let corner = vec2i(floor(t));
    let f = t - floor(t);
    var lights: array<vec3f, 4>;
    for (var i = 0; i < 4; i += 1) {
        let n = pixel_light(input, corner + vec2i(i & 1, i >> 1u));
        lights[i] = select(own.rgb, n.rgb, n.w == 1.0);
    }
    let light = mix(mix(lights[0], lights[1], f.x), mix(lights[2], lights[3], f.x), f.y);

    var rgb = srgb_decode(hd.rgb) * light;
    if params.frame.w == 1.0 {
        rgb = mix(rgb, vec3f(1.0, 0.0, 1.0), 0.35);
    }
    return vec4f(rgb, 1.0);
}
