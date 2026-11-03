// AmbientLight (Ganossa, 2015). Detects the frame's bright areas, blurs them
// through twelve horizontal/vertical 5-tap pairs at 1/16 resolution, adapts the
// mix to the average scene luminance, and blends the lit result over the frame.
//
// Ported without the lens and dirt-texture arms (the effect reads Dirt.png and
// Lens*.png, which the game does not ship): the dirt term keeps the effect's
// texture-free branch (`highOrigin * high * Dirt.Intensity`), and `AL_Lens` is
// not exposed. The debug bars are dropped.
//
// UI texels outside the tactical viewport keep the graded frame.

struct Params {
    dst: vec2f,   // render target size in pixels
    frame: vec2f, // graded frame size in pixels
    step: vec2f,  // one texel of the 1/16 buffers, in uv
    phase: f32,   // the effect's ping-pong time (0..2*pi)
    intensity: f32,
    threshold: f32,
    adapt_strength: f32,
    adapt_base_mult: f32,
    black_level: f32,
    dirt_intensity: f32,
    adaptation: f32,
    dither: f32,
    dirt: f32,
    viewport: vec4f,
};

@group(0) @binding(0) var t_source: texture_2d<f32>;
@group(0) @binding(1) var s_linear: sampler;
@group(0) @binding(2) var<uniform> params: Params;
@group(0) @binding(3) var t_aux: texture_2d<f32>;
@group(0) @binding(4) var t_low: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn outside_viewport(position: vec2f) -> bool {
    let sample_position = position * params.frame;
    let v = params.viewport;
    return sample_position.x < v.x
        || sample_position.y < v.y
        || sample_position.x >= v.x + v.z
        || sample_position.y >= v.y + v.w;
}

/// The 1/16-resolution copy the highlight detection reads.
@fragment
fn fs_detect_int(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / params.dst;
    return textureSampleLevel(t_source, s_linear, uv, 0.0);
}

/// 1x1 average of that copy.
@fragment
fn fs_detect_low(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let dims = vec2i(textureDimensions(t_source));
    var sum = vec3f(0.0);
    for (var y = 0; y < dims.y; y += 1) {
        for (var x = 0; x < dims.x; x += 1) {
            sum += textureLoad(t_source, vec2i(x, y), 0).rgb;
        }
    }
    let count = f32(dims.x * dims.y);
    return vec4f(sum / count, 1.0);
}

/// The effect's highlight extraction: keep the colour, weight it by the
/// strongest channel squared, then pull the channels back into 0..1 without
/// losing the ratios.
@fragment
fn fs_detect_high(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / params.dst;
    let source = textureSampleLevel(t_source, s_linear, uv, 0.0).rgb;
    let strongest = max(source.r, max(source.g, source.b));
    let x = source * pow(abs(strongest), 2.0);
    let base = (x.r + x.g + x.b) / 3.0;
    var n = vec3f(x.r * 2.0 - base, x.g * 2.0 - base, x.b * 2.0 - base);
    // Pull negative channels up by borrowing from the others.
    if (n.r < 0.0) {
        n.g += n.r / 2.0;
        n.b += n.r / 2.0;
        n.r = 0.0;
    }
    if (n.g < 0.0) {
        n.b += n.g / 2.0;
        if (n.r > -n.g / 2.0) {
            n.r += n.g / 2.0;
        } else {
            n.r = 0.0;
        }
        n.g = 0.0;
    }
    if (n.b < 0.0) {
        if (n.r > -n.b / 2.0) {
            n.r += n.b / 2.0;
        } else {
            n.r = 0.0;
        }
        if (n.g > -n.b / 2.0) {
            n.g += n.b / 2.0;
        } else {
            n.g = 0.0;
        }
        n.b = 0.0;
    }
    // And clamp overflows, handing the excess to the other channels.
    if (n.r > 1.0) {
        n.g += (n.r - 1.0) / 2.0;
        n.b += (n.r - 1.0) / 2.0;
        n.r = 1.0;
    }
    if (n.g > 1.0) {
        n.b += (n.g - 1.0) / 2.0;
        if (n.r + (n.g - 1.0) < 1.0) {
            n.r += (n.g - 1.0) / 2.0;
        } else {
            n.r = 1.0;
        }
        n.g = 1.0;
    }
    if (n.b > 1.0) {
        if (n.r + (n.b - 1.0) < 1.0) {
            n.r += (n.b - 1.0) / 2.0;
        } else {
            n.r = 1.0;
        }
        if (n.g + (n.b - 1.0) < 1.0) {
            n.g += (n.b - 1.0) / 2.0;
        } else {
            n.g = 1.0;
        }
        n.b = 1.0;
    }
    return vec4f(n, 1.0);
}

const SAMPLE_OFFSETS: array<f32, 5> = array<f32, 5>(0.0, 2.4347826, 4.3478260, 6.2608695, 8.1739130);
const SAMPLE_WEIGHTS: array<f32, 5> = array<f32, 5>(0.16818994, 0.27276957, 0.111690125, 0.024067905, 0.0021112196);

/// The horizontal blur pair: the centre term is thresholded, the neighbours are
/// not, so lights spread sideways while the flat background stays out.
@fragment
fn fs_blur_h(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / params.dst;
    var color = textureSampleLevel(t_source, s_linear, uv, 0.0).rgb * SAMPLE_WEIGHTS[0];
    color = max(color - vec3f(params.threshold), vec3f(0.0));
    let step_mult = 1.08 + (params.phase / 100.0) * 0.02;
    for (var tap = 1; tap < 5; tap += 1) {
        let offset = SAMPLE_OFFSETS[tap];
        color += textureSampleLevel(t_source, s_linear, uv + vec2f(offset * params.step.x, 0.0), 0.0).rgb
            * SAMPLE_WEIGHTS[tap]
            * step_mult;
        color += textureSampleLevel(t_source, s_linear, uv - vec2f(offset * params.step.x, 0.0), 0.0).rgb
            * SAMPLE_WEIGHTS[tap]
            * step_mult;
    }
    return vec4f(color, 1.0);
}

/// The vertical half of that pair.
@fragment
fn fs_blur_v(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / params.dst;
    var color = textureSampleLevel(t_source, s_linear, uv, 0.0).rgb * SAMPLE_WEIGHTS[0];
    color = max(color - vec3f(params.threshold), vec3f(0.0));
    let step_mult = 1.08 + (params.phase / 100.0) * 0.02;
    for (var tap = 1; tap < 5; tap += 1) {
        let offset = SAMPLE_OFFSETS[tap];
        color += textureSampleLevel(t_source, s_linear, uv + vec2f(0.0, offset * params.step.y), 0.0).rgb
            * SAMPLE_WEIGHTS[tap]
            * step_mult;
        color += textureSampleLevel(t_source, s_linear, uv - vec2f(0.0, offset * params.step.y), 0.0).rgb
            * SAMPLE_WEIGHTS[tap]
            * step_mult;
    }
    return vec4f(color, 1.0);
}

/// The final mix: adapted ambient light over the frame, using the effect's
/// texture-free dirt branch.
@fragment
fn fs_magic(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let dims = vec2f(textureDimensions(t_source));
    let uv = position.xy / dims;
    let base_raw = textureLoad(t_source, vec2i(position.xy), 0).rgb;
    if (outside_viewport(uv)) {
        return vec4f(base_raw, 1.0);
    }
    var high = min(textureSampleLevel(t_aux, s_linear, uv, 0.0).rgb, vec3f(0.0325)) * 1.15;
    let high_origin = high;
    if (params.dirt > 0.5) {
        high = high + high_origin * high * params.dirt_intensity;
    }

    var adapt = 0.0;
    if (params.adaptation > 0.5) {
        let low = textureLoad(t_low, vec2i(0, 0), 0).rgb / 4.215;
        var luminance = sqrt(0.241 * low.r * low.r + 0.691 * low.g * low.g + 0.068 * low.b * low.b);
        luminance = pow(luminance * 1.25, 2.0);
        adapt = luminance * (luminance + 1.0) * params.adapt_strength * params.intensity * 5.0;
    }

    var dither = 0.0;
    if (params.dither > 0.5) {
        let base_dither = 0.15 * (1.0 / (1024.0 - 1.0));
        let noise = fract(dot(uv, params.frame * vec2f(1.0 / 16.0, 10.0 / 36.0)) + 0.25);
        dither = mix(2.0 * base_dither, -2.0 * base_dither, noise);
    }

    if (all(base_raw == vec3f(1.0))) {
        return vec4f(1.0);
    }

    var base = base_raw;
    var mix_amount = 0.0;
    if (params.adaptation > 0.5) {
        base = base * max(
            0.0,
            1.0 - adapt * 0.75 * params.adapt_base_mult
                * pow(abs(1.0 - (base.r + base.g + base.b) / 3.0), params.black_level),
        );
        mix_amount = max(0.0, params.intensity - adapt);
    } else {
        mix_amount = params.intensity;
    }
    let high_sample = (vec3f(1.0) - (vec3f(1.0) - base) * (vec3f(1.0) - high)) + vec3f(dither);
    let sample = mix(base, high_sample, mix_amount);
    let sample_sum = sample.r + sample.g + sample.b;
    if (sample_sum > 0.008) {
        return vec4f(sample, 1.0);
    }
    let fallback = mix(base, high_sample, max(0.0, mix_amount * 0.85) * sample_sum);
    return vec4f(fallback, 1.0);
}
