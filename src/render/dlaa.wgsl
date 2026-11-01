// Spatial half of Barbatos' `BaBa_DLAA-T.fx`: Directionally Localized AA
// (Andreev, GDC 2011), with the optional temporal pass left out.
//
// `fs_prefilter` writes the frame's gamma-space colour plus a crushed
// high-pass alpha; `fs_spatial` reads that map, builds the short/long edge
// masks and blends the dominant axis back into the centre texel.
//
// The effect is authored in gamma space, so the prefilter encodes and the
// spatial pass decodes for the sRGB target. Texels outside the tactical
// viewport come back untouched, so UI chrome keeps its bytes.

struct Params {
    // strength, lambda, epsilon, edge contrast
    scalars: vec4f,
    // long-edge threshold, unused, unused, unused
    gate: vec4f,
    // (x, y, width, height) of the tactical viewport in frame pixels.
    viewport: vec4f,
};

@group(0) @binding(0) var t_source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var s_linear: sampler;

fn linear_to_srgb(c: vec3f) -> vec3f {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3f(0.0031308));
}

fn srgb_to_linear(c: vec3f) -> vec3f {
    let x = clamp(c, vec3f(0.0), vec3f(1.0));
    let lo = x / 12.92;
    let hi = pow((x + 0.055) / 1.055, vec3f(2.4));
    return select(hi, lo, x <= vec3f(0.04045));
}

struct ShortMasks {
    blurred_h: vec4f,
    blurred_v: vec4f,
    sat_h: f32,
    sat_v: f32,
};

fn load_pf(uv: vec2f, offset_px: vec2f, texel: vec2f) -> vec4f {
    return textureSampleLevel(t_source, s_linear, uv + offset_px * texel, 0.0);
}

// Talk p43 / TFU2 (5-tap H/V). Green is the lightness channel (`LI`).
fn short_edge(
    center: vec4f,
    left: vec4f,
    right: vec4f,
    up: vec4f,
    down: vec4f,
    lambda: f32,
    epsilon: f32,
) -> ShortMasks {
    let comb_h = 2.0 * (left + right);
    let comb_v = 2.0 * (up + down);
    let diff_h = abs(comb_h - 4.0 * center) / 4.0;
    let diff_v = abs(comb_v - 4.0 * center) / 4.0;
    var masks: ShortMasks;
    masks.blurred_h = (comb_h + 2.0 * center) / 6.0;
    masks.blurred_v = (comb_v + 2.0 * center) / 6.0;
    let lum_h = diff_h.g;
    let lum_v = diff_v.g;
    let lum_hb = max(masks.blurred_h.g, 1e-4);
    let lum_vb = max(masks.blurred_v.g, 1e-4);
    masks.sat_h = clamp((lambda * lum_h - epsilon) / lum_vb, 0.0, 1.0);
    masks.sat_v = clamp((lambda * lum_v - epsilon) / lum_hb, 0.0, 1.0);
    return masks;
}

struct LongMasks {
    mask_h: f32,
    mask_v: f32,
    gate: f32,
    lb_h: f32,
    lb_v: f32,
};

// Talk p44: sparse long-edge taps reuse the ±1 ring plus ±3.5/5.5/7.5.
fn long_edge(
    uv: vec2f,
    left: vec4f,
    right: vec4f,
    up: vec4f,
    down: vec4f,
    texel: vec2f,
    threshold: f32,
) -> LongMasks {
    let h_neg_a = load_pf(uv, vec2f(-3.5, 0.0), texel);
    let h_neg_b = load_pf(uv, vec2f(-5.5, 0.0), texel);
    let h_neg_c = load_pf(uv, vec2f(-7.5, 0.0), texel);
    let h_pos_a = load_pf(uv, vec2f(3.5, 0.0), texel);
    let h_pos_b = load_pf(uv, vec2f(5.5, 0.0), texel);
    let h_pos_c = load_pf(uv, vec2f(7.5, 0.0), texel);
    let v_neg_a = load_pf(uv, vec2f(0.0, -3.5), texel);
    let v_neg_b = load_pf(uv, vec2f(0.0, -5.5), texel);
    let v_neg_c = load_pf(uv, vec2f(0.0, -7.5), texel);
    let v_pos_a = load_pf(uv, vec2f(0.0, 3.5), texel);
    let v_pos_b = load_pf(uv, vec2f(0.0, 5.5), texel);
    let v_pos_c = load_pf(uv, vec2f(0.0, 7.5), texel);

    let avg_h = (left + h_neg_a + h_neg_b + h_neg_c + right + h_pos_a + h_pos_b + h_pos_c) / 8.0;
    let avg_v = (up + v_neg_a + v_neg_b + v_neg_c + down + v_pos_a + v_pos_b + v_pos_c) / 8.0;

    var masks: LongMasks;
    masks.mask_h = clamp(avg_h.a * 2.0 - 1.0, 0.0, 1.0);
    masks.mask_v = clamp(avg_v.a * 2.0 - 1.0, 0.0, 1.0);
    masks.gate = select(0.0, 1.0, abs(masks.mask_h - masks.mask_v) > threshold);
    masks.lb_h = avg_h.g;
    masks.lb_v = avg_v.g;
    return masks;
}

// A valid `t` in (0,1) for the luminosity match; 0 is invalid.
fn safe_luma_t(blurred: f32, from_lum: f32, to_lum: f32) -> f32 {
    let denom = to_lum - from_lum;
    if (abs(denom) < 1e-4) {
        return 0.0;
    }
    let t = (blurred - from_lum) / denom;
    return select(0.0, t, t > 0.0 && t < 1.0);
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_prefilter(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let dims = vec2f(textureDimensions(t_source));
    let uv = position.xy / dims;
    let texel = 1.0 / dims;
    let center = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv, 0.0).rgb);
    let left = linear_to_srgb(
        textureSampleLevel(t_source, s_linear, uv + vec2f(-texel.x, 0.0), 0.0).rgb,
    );
    let right = linear_to_srgb(
        textureSampleLevel(t_source, s_linear, uv + vec2f(texel.x, 0.0), 0.0).rgb,
    );
    let up = linear_to_srgb(
        textureSampleLevel(t_source, s_linear, uv + vec2f(0.0, -texel.y), 0.0).rgb,
    );
    let down = linear_to_srgb(
        textureSampleLevel(t_source, s_linear, uv + vec2f(0.0, texel.y), 0.0).rgb,
    );
    // High-pass + crush (talk slide 22): saturate(abs(x) * a - b).
    let edges = 4.0 * abs((left + right + up + down) - 4.0 * center);
    let crushed = clamp(edges.g * params.scalars.w - params.scalars.z, 0.0, 1.0);
    return vec4f(center, crushed);
}

@fragment
fn fs_spatial(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let dims = vec2f(textureDimensions(t_source));
    let uv = position.xy / dims;
    let texel = 1.0 / dims;
    let center = load_pf(uv, vec2f(0.0, 0.0), texel);
    let left = load_pf(uv, vec2f(-1.0, 0.0), texel);
    let right = load_pf(uv, vec2f(1.0, 0.0), texel);
    let up = load_pf(uv, vec2f(0.0, -1.0), texel);
    let down = load_pf(uv, vec2f(0.0, 1.0), texel);

    let short = short_edge(
        center, left, right, up, down, params.scalars.y, params.scalars.z,
    );
    var dlaa = center;
    dlaa = mix(dlaa, short.blurred_h, short.sat_v);
    // Second axis at half weight.
    dlaa = mix(dlaa, short.blurred_v, short.sat_h * 0.5);

    let long = long_edge(uv, left, right, up, down, texel, params.gate.x);
    // Only one dominant axis, and only where the mask is actually strong.
    if (long.gate > 0.0 && max(long.mask_h, long.mask_v) > 0.15) {
        let c_l = center.g;
        var clr_h = center;
        let t_up = safe_luma_t(long.lb_h, up.g, c_l);
        let t_dn = safe_luma_t(long.lb_h, c_l, down.g);
        if (t_up > 0.0) {
            clr_h = mix(up, center, t_up);
        } else if (t_dn > 0.0) {
            clr_h = mix(center, down, t_dn);
        }

        var clr_v = center;
        let t_lf = safe_luma_t(long.lb_v, left.g, c_l);
        let t_rt = safe_luma_t(long.lb_v, c_l, right.g);
        if (t_lf > 0.0) {
            clr_v = mix(left, center, t_lf);
        } else if (t_rt > 0.0) {
            clr_v = mix(center, right, t_rt);
        }

        if (long.mask_h > long.mask_v) {
            dlaa = mix(dlaa, clr_h, long.mask_h);
        } else {
            dlaa = mix(dlaa, clr_v, long.mask_v);
        }
    }

    let p = position.xy;
    let v = params.viewport;
    if (p.x < v.x || p.y < v.y || p.x >= v.x + v.z || p.y >= v.y + v.w) {
        return vec4f(srgb_to_linear(center.rgb), 1.0);
    }
    return vec4f(srgb_to_linear(mix(center.rgb, dlaa.rgb, params.scalars.x)), 1.0);
}
