// VERA's Modern picture, VERA-only: no gamemd counterpart. One pass over the
// finished frame: a glow gathered from the bright pixels around each pixel,
// an unsharp mask, then contrast and saturation.
//
// `srgb_encode` / `srgb_decode` are prepended by modern_graphics.rs. The
// source and target are ordinary sRGB views, so loads are linear: the glow
// adds light in linear space, and the grade works on encoded values, where
// the eye's steps are even.

@group(0) @binding(0) var source: texture_2d<f32>;

struct Look {
    // x: glow strength, y: glow threshold (linear luminance),
    // z: glow radius in pixels, w: sharpen amount
    glow: vec4f,
    // x: contrast, y: saturation, z: unused, w: unused
    grade: vec4f,
    // The battlefield in target pixels: left, top, right, bottom (exclusive).
    area: vec4f,
};
@group(0) @binding(1) var<uniform> look: Look;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let corner = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(corner * 2.0 - 1.0, 0.0, 1.0);
}

// Reads stay on the battlefield, so the sidebar's light never glows into it.
fn texel(at: vec2i) -> vec3f {
    let first = vec2i(look.area.xy);
    let last = min(vec2i(look.area.zw) - 1, vec2i(textureDimensions(source)) - 1);
    return textureLoad(source, clamp(at, first, last), 0).rgb;
}

fn luminance(rgb: vec3f) -> f32 {
    return dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
}

// The part of a pixel's light above the threshold.
fn bright(rgb: vec3f) -> vec3f {
    let over = max(luminance(rgb) - look.glow.y, 0.0);
    return rgb * (over / (over + 0.25));
}

const GOLDEN_ANGLE: f32 = 2.39996323;
const GLOW_TAPS: i32 = 40;

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let at = vec2i(position.xy);
    let centre = texel(at);

    // Unsharp mask against the four neighbours.
    let around = (texel(at + vec2i(1, 0)) + texel(at - vec2i(1, 0))
        + texel(at + vec2i(0, 1)) + texel(at - vec2i(0, 1))) * 0.25;
    var rgb = max(centre + (centre - around) * look.glow.w, vec3f(0.0));

    // Glow: bright pixels on a golden-angle spiral, nearer ones weigh more.
    var glow = vec3f(0.0);
    var weight = 0.0;
    for (var i = 0; i < GLOW_TAPS; i += 1) {
        let t = (f32(i) + 0.5) / f32(GLOW_TAPS);
        let radius = sqrt(t) * look.glow.z;
        let angle = f32(i) * GOLDEN_ANGLE;
        let offset = vec2i(round(vec2f(cos(angle), sin(angle)) * radius));
        let w = 1.0 - t * 0.75;
        glow += bright(texel(at + offset)) * w;
        weight += w;
    }
    rgb += glow / weight * look.glow.x;

    // Grade on encoded values: contrast about mid grey, then saturation.
    var encoded = srgb_encode(clamp(rgb, vec3f(0.0), vec3f(1.0)));
    encoded = (encoded - 0.5) * look.grade.x + 0.5;
    let grey = luminance(encoded);
    encoded = mix(vec3f(grey), encoded, look.grade.y);
    return vec4f(srgb_decode(clamp(encoded, vec3f(0.0), vec3f(1.0))), 1.0);
}
