// Temporal accumulation of the reduced world, before the scaled copy.
//
// The current low-resolution frame is blended into a history reprojected by the
// camera's integer-pixel delta. The history is clamped to the current 3x3
// neighbourhood so a disoccluded or moving object cannot leave a trail, which
// is the cheap stand-in for FSR2's depth/motion-vector disocclusion.
//
// Coordinates are source texels: a world pixel lands at
// `(world - camera) * render_scale` source texels, independent of wheel zoom,
// so the reprojection only needs the camera delta and the render scale.

struct Params {
    src_size: vec2f, // source texels, float
    alpha: f32,      // current-frame weight; lower keeps more history
    reset: f32,      // 1 starts the history from this frame
    delta: vec2f,    // history uv offset, `(camera_now - camera_prev) * scale / size`
    _pad: vec2f,
};

@group(0) @binding(0) var t_current: texture_2d<f32>;
@group(0) @binding(1) var t_history: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: Params;
@group(0) @binding(3) var s_linear: sampler;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn current_at(at: vec2i) -> vec4f {
    let last = vec2i(textureDimensions(t_current)) - 1;
    return textureLoad(t_current, clamp(at, vec2i(0), last), 0);
}

@fragment
fn fs_resolve(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let pi = vec2i(position.xy);
    let current = current_at(pi);
    if (params.reset > 0.5) {
        return current;
    }
    let uv = position.xy / params.src_size;
    let uv_prev = uv + params.delta;
    if (uv_prev.x < 0.0 || uv_prev.y < 0.0 || uv_prev.x > 1.0 || uv_prev.y > 1.0) {
        return current;
    }
    let history = textureSampleLevel(t_history, s_linear, uv_prev, 0.0).rgb;
    // Clamp the reprojection to what this frame actually shows: an object that
    // moved out of the neighbourhood cannot bleed back in.
    var minimum = current.rgb;
    var maximum = current.rgb;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let texel = current_at(pi + vec2i(x, y)).rgb;
            minimum = min(minimum, texel);
            maximum = max(maximum, texel);
        }
    }
    let clamped = clamp(history, minimum, maximum);
    return vec4f(mix(clamped, current.rgb, params.alpha), 1.0);
}
