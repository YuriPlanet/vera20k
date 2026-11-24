// SurfaceExt::BlurRect's three-box gaussian (`Ext/Surface/Body.cpp:9`), one
// box pass per invocation: the fragment averages a clamped texel window
// along `direction`, the six sequential passes (three boxes, horizontal then
// vertical each) reproduce the native ping-pong. The composite mode copies
// the finished blur back over the rect in the composition.
struct BlurUniforms {
    // The source texture's pixel size.
    source_size: vec2<f32>,
    // The blurred rect's origin inside the source, in pixels.
    rect_origin: vec2<f32>,
    // This pass's box radius in pixels and its direction (0 x, 1 y).
    radius_direction: vec2<f32>,
    // The quad's clip-space window: the full target for a blur pass, the
    // rect's window for the composite.
    clip_min: vec2<f32>,
    clip_max: vec2<f32>,
    // 0 blur, 1 composite copy of the finished blur.
    mode_radius: vec2<f32>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var tex: sampler;
@group(0) @binding(2) var<uniform> u: BlurUniforms;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) texcoord: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vertex: u32) -> VertexOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let corner = corners[vertex];
    let clip = u.clip_min + corner * (u.clip_max - u.clip_min);
    var out: VertexOut;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.texcoord = corner;
    return out;
}

@fragment
fn fs(out: VertexOut) -> @location(0) vec4<f32> {
    if (u.mode_radius.y > 0.5) {
        // The composite copy: the blur target is exactly the quad.
        return textureSampleLevel(source, tex, out.texcoord, 0.0);
    }
    let pixel = floor(u.rect_origin + out.texcoord * u.rect_size);
    let texel = 1.0 / u.source_size;
    let radius = i32(u.radius_direction.x);
    var sum = vec3<f32>(0.0, 0.0, 0.0);
    for (var offset = -radius; offset <= radius; offset = offset + 1) {
        let step = select(
            vec2<f32>(f32(offset), 0.0),
            vec2<f32>(0.0, f32(offset)),
            u.radius_direction.y > 0.5,
        );
        // `textureSampleLevel` on a clamp-to-edge sampler reproduces the
        // native sliding window's boundary pixels (`fv`/`lv` at :45-46).
        let sample_uv = (pixel + step) * texel;
        sum = sum + textureSampleLevel(source, tex, sample_uv, 0.0).rgb;
    }
    return vec4<f32>(sum / f32(2 * radius + 1), 1.0);
}
