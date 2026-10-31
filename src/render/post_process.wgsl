// The presentation hook's frame pass.
//
// `textureLoad`, never a sampler: with no effects configured this pass must
// reproduce the 1:1 texture copy it replaces bit for bit, so it reads the
// texel the fragment centre lands in and applies no filtering, no half-texel
// offset and no colour conversion the copy would not also apply.
//
// The optional colour-grade arm runs in linear RGB (the sRGB target decodes on
// load and encodes on store). With it off the loaded texel is returned
// untouched.

struct Params {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    // 0 = no colour grade, 1 = colour grade.
    color_grade: f32,
    // (x, y, width, height) of the tactical viewport in frame pixels. Texels
    // outside it (sidebar column, bottom strip) return unchanged.
    viewport: vec4f,
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

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let source = textureLoad(t_source, vec2i(position.xy), 0);

    if (params.color_grade == 0.0) {
        return source;
    }

    let p = position.xy;
    let v = params.viewport;
    if (p.x < v.x || p.y < v.y || p.x >= v.x + v.z || p.y >= v.y + v.w) {
        return source;
    }

    var color = source;
    var rgb = color.rgb;
    let luma = dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
    rgb = mix(vec3f(luma), rgb, params.saturation);
    rgb = (rgb - vec3f(0.5)) * params.contrast + vec3f(0.5);
    rgb = rgb + vec3f(params.brightness);
    color = vec4f(clamp(rgb, vec3f(0.0), vec3f(1.0)), color.a);
    return color;
}
