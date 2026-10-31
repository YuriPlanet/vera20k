// The presentation hook's frame pass.
//
// `textureLoad`, never a sampler: with no effects configured this pass must
// reproduce the 1:1 texture copy it replaces bit for bit, so it reads the
// texel the fragment centre lands in and applies no filtering, no half-texel
// offset and no colour conversion the copy would not also apply.
//
// The optional arms:
//   - ColorGrade runs in linear RGB on the decoded texel;
//   - FXAA refetches neighbours through the filtering sampler and blends edge
//     texels first, so the graded result already has its antialiasing applied;
//   - FilmicPass, Colourfulness and Sepia are ports of the ReShade community
//     shaders of those names, which are authored in gamma space. They run after
//     an explicit linear-to-sRGB encode and the result is decoded back for the
//     sRGB target.
// With every arm off the loaded texel is returned untouched.

struct Params {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    // 0 = no colour grade, 1 = colour grade.
    color_grade: f32,

    // (x, y, width, height) of the tactical viewport in frame pixels. Texels
    // outside it (sidebar column, bottom strip) return unchanged.
    viewport: vec4f,

    filmic_strength: f32,
    filmic_fade: f32,
    filmic_contrast: f32,
    filmic_linearization: f32,

    filmic_bleach: f32,
    filmic_saturation: f32,
    // 0 = no FilmicPass, 1 = FilmicPass.
    filmic_enabled: f32,
    pad0: f32,

    colour_amount: f32,
    colour_lim_luma: f32,
    // 0 = no Colourfulness, 1 = Colourfulness.
    colour_enabled: f32,
    // 0 = no Sepia, 1 = Sepia.
    sepia_enabled: f32,

    sepia_strength: f32,
    sepia_tint_r: f32,
    sepia_tint_g: f32,
    sepia_tint_b: f32,

    fxaa_subpix: f32,
    fxaa_edge_threshold: f32,
    fxaa_edge_threshold_min: f32,
    // 0 = no FXAA, 1 = FXAA.
    fxaa_enabled: f32,

    // Bloom is applied by its own passes; these keep the layout in step with
    // the shared uniform.
    bloom_threshold: f32,
    bloom_intensity: f32,
    bloom_radius: f32,
    bloom_enabled: f32,

    ripple_amplitude: f32,
    ripple_wavelength: f32,
    ripple_speed: f32,
    // Seconds since presentation started.
    ripple_time: f32,

    // 0 = no displacement, 1 = displacement over the water mask.
    ripple_enabled: f32,
    // 1 tints masked texels magenta instead of refracting them.
    mask_debug: f32,
    pad1: f32,
    pad2: f32,
};

@group(0) @binding(0) var t_source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var s_linear: sampler;
// Engine-built water mask: 1 where the cell's land type is Water.
@group(0) @binding(3) var t_mask: texture_2d<f32>;

// ReShade `FilmicPass.fx` luminance coefficients.
const FILMIC_LUMA = vec3f(0.212656, 0.715158, 0.072186);
// ReShade `Colourfulness.fx` mean of Rec. 709 & 601 luma coefficients.
const COLOUR_LUMA = vec3f(0.2558, 0.6511, 0.0931);
// FilmicPass curve/gamma constants left at their authored defaults.
const FILMIC_CURVE = 1.0;
const FILMIC_BASE_CURVE = 1.5;
const FILMIC_EFFECT_GAMMA = 0.65;

fn sigmoid(x: f32) -> f32 {
    return 1.0 / (1.0 + exp(-x));
}

fn curve_norm(curve: f32) -> f32 {
    return 1.0 / (1.0 + exp(curve / 2.0));
}

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

// The quadratic/sqrt toe used by FilmicPass for a single channel.
fn filmic_tone(x: f32, b: f32) -> f32 {
    if (x < 0.5) {
        return (2.0 * x - 1.0) * (b - b * b) + b;
    }
    return (2.0 * x - 1.0) * (sqrt(b) - b) + b;
}

// Port of ReShade `FilmicPass.fx`; curve/gamma constants stay at defaults.
fn filmic_pass(b0: vec3f) -> vec3f {
    let h = vec3f(0.01);
    var b = clamp(b0, vec3f(0.0), vec3f(1.0));
    b = pow(b, vec3f(params.filmic_linearization));
    b = h + (b - h) * params.filmic_contrast;

    // BaseGamma is 1.0, so the base keeps its values.
    let a_luma = dot(b, FILMIC_LUMA);
    var d = vec3f(a_luma);
    let y = curve_norm(FILMIC_CURVE);
    d.x = (sigmoid(FILMIC_CURVE * (d.x - 0.5)) - y) / (1.0 - 2.0 * y);
    d.y = (sigmoid(FILMIC_CURVE * (d.y - 0.5)) - y) / (1.0 - 2.0 * y);
    d.z = (sigmoid(FILMIC_CURVE * (d.z - 0.5)) - y) / (1.0 - 2.0 * y);

    d = pow(abs(d), vec3f(1.0 / FILMIC_EFFECT_GAMMA));
    d = mix(d, vec3f(1.0) - d, params.filmic_bleach);

    var c = vec3f(
        filmic_tone(d.x, b.x),
        filmic_tone(d.y, b.y),
        filmic_tone(d.z, b.z),
    );
    var f = mix(b, c, params.filmic_strength);

    let v = curve_norm(FILMIC_BASE_CURVE);
    f = vec3f(
        (sigmoid(FILMIC_BASE_CURVE * (f.x - 0.5)) - v) / (1.0 - 2.0 * v),
        (sigmoid(FILMIC_BASE_CURVE * (f.y - 0.5)) - v) / (1.0 - 2.0 * v),
        (sigmoid(FILMIC_BASE_CURVE * (f.z - 0.5)) - v) / (1.0 - 2.0 * v),
    );

    let sat = params.filmic_saturation;
    let fade = params.filmic_fade;
    let r2r = 1.0 - sat;
    let g2r = sat;
    let b2r = sat;
    let r2g = sat;
    let g2g = (1.0 - fade) - sat;
    let b2g = fade + sat;
    let r2b = sat;
    let g2b = fade + sat;
    let b2b = (1.0 - fade) - sat;
    let i_f = f;
    f = vec3f(
        i_f.x * r2r + i_f.y * g2r + i_f.z * b2r,
        i_f.x * r2g + i_f.y * g2g + i_f.z * b2g,
        i_f.x * r2b + i_f.y * g2b + i_f.z * b2b,
    );

    let n = dot(f, FILMIC_LUMA);
    c = vec3f(
        filmic_tone(n, f.x),
        filmic_tone(n, f.y),
        filmic_tone(n, f.z),
    );
    c = pow(max(c, vec3f(0.0)), vec3f(1.0 / params.filmic_linearization));
    return mix(b, c, params.filmic_strength);
}

// Port of ReShade `Colourfulness.fx` (dither off).
fn colourfulness_pass(c_in: vec3f) -> vec3f {
    let c0 = clamp(c_in, vec3f(0.0), vec3f(1.0));
    let luma = sqrt(dot(c0 * abs(c0), COLOUR_LUMA));
    let diff = c0 - vec3f(luma);
    var c_diff = diff * (params.colour_amount + 1.0) - diff;

    if (params.colour_amount > 0.0) {
        let rlc = clamp(c_diff * 1.2 + c0, vec3f(-0.0001), vec3f(1.0001)) - c0;
        let max_diff = max(max(diff.x, diff.y), diff.z);
        let min_diff = min(min(diff.x, diff.y), diff.z);
        let pos_lim = (1.0002 - luma) / (abs(max_diff) + 0.0001);
        let neg_lim = (luma + 0.0002) / (abs(min_diff) + 0.0001);
        let lim_factor = min(min(pos_lim, neg_lim), 32.0);
        let diff_max = diff * lim_factor - diff;

        let w = clamp(params.colour_lim_luma, 0.0, 1.0);
        let mean = pow(abs(w) * sqrt(abs(diff_max)) + abs(1.0 - w) * sqrt(abs(rlc)), vec3f(2.0));
        let soft = max(mean, vec3f(1e-7));
        c_diff = (c_diff * soft) / sqrt(soft * soft + c_diff * c_diff);
    }
    return clamp(c0 + c_diff, vec3f(0.0), vec3f(1.0));
}

// Port of ReShade `Sepia.fx` (`technique Tint`).
fn sepia_pass(c: vec3f) -> vec3f {
    let tint = vec3f(params.sepia_tint_r, params.sepia_tint_g, params.sepia_tint_b);
    return mix(c, c * tint * 2.55, params.sepia_strength);
}

// ReShade `FXAA.fx` luma, computed in gamma space.
fn fxaa_luma(c: vec3f) -> f32 {
    return sqrt(dot(c * c, vec3f(0.299, 0.587, 0.114)));
}

fn fxaa_sample(uv: vec2f) -> vec3f {
    return linear_to_srgb(textureSampleLevel(t_source, s_linear, uv, 0.0).rgb);
}

// Port of ReShade `FXAA.fx` (FXAA 3.11 quality, preset 15) with the dither
// gather path replaced by plain samples, as the 32-bit Direct3D 9 path uses.
fn fxaa_pass(center: vec3f, uv: vec2f) -> vec3f {
    let dims = vec2f(textureDimensions(t_source));
    let rcp = vec2f(1.0 / dims.x, 1.0 / dims.y);

    let south = fxaa_sample(uv + vec2f(0.0, rcp.y));
    let east = fxaa_sample(uv + vec2f(rcp.x, 0.0));
    let north = fxaa_sample(uv + vec2f(0.0, -rcp.y));
    let west = fxaa_sample(uv + vec2f(-rcp.x, 0.0));

    let lumaM = fxaa_luma(center);
    let lumaS = fxaa_luma(south);
    let lumaE = fxaa_luma(east);
    let lumaN = fxaa_luma(north);
    let lumaW = fxaa_luma(west);

    let rangeMax = max(max(lumaN, lumaW), max(lumaE, max(lumaS, lumaM)));
    let rangeMin = min(min(lumaN, lumaW), min(lumaE, min(lumaS, lumaM)));
    let range = rangeMax - rangeMin;
    if (range < max(params.fxaa_edge_threshold_min, rangeMax * params.fxaa_edge_threshold)) {
        return center;
    }

    let lumaNW = fxaa_luma(fxaa_sample(uv + vec2f(-rcp.x, -rcp.y)));
    let lumaNE = fxaa_luma(fxaa_sample(uv + vec2f(rcp.x, -rcp.y)));
    let lumaSW = fxaa_luma(fxaa_sample(uv + vec2f(-rcp.x, rcp.y)));
    let lumaSE = fxaa_luma(fxaa_sample(uv + vec2f(rcp.x, rcp.y)));

    let edgeHorz1 = (-2.0 * lumaM) + (lumaN + lumaS);
    let edgeVert1 = (-2.0 * lumaM) + (lumaW + lumaE);
    let edgeHorz2 = (-2.0 * lumaE) + (lumaNE + lumaSE);
    let edgeVert2 = (-2.0 * lumaN) + (lumaNW + lumaNE);
    let edgeHorz3 = (-2.0 * lumaW) + (lumaNW + lumaSW);
    let edgeVert3 = (-2.0 * lumaS) + (lumaSW + lumaSE);
    let edgeHorz = abs(edgeHorz3) + abs(edgeHorz1) * 2.0 + abs(edgeHorz2);
    let edgeVert = abs(edgeVert3) + abs(edgeVert1) * 2.0 + abs(edgeVert2);

    let subpixNSWE = lumaN + lumaS + lumaW + lumaE;
    let subpixNWSWNESE = (lumaNW + lumaSW) + (lumaNE + lumaSE);
    var horzSpan = edgeHorz >= edgeVert;
    var lumaNv = lumaN;
    var lumaSv = lumaS;
    var lengthSign = rcp.x;
    if (!horzSpan) {
        lumaNv = lumaW;
        lumaSv = lumaE;
    }
    if (horzSpan) {
        lengthSign = rcp.y;
    }

    let subpixB = (subpixNSWE * 2.0 + subpixNWSWNESE) * (1.0 / 12.0) - lumaM;
    let gradientN = lumaNv - lumaM;
    let gradientS = lumaSv - lumaM;
    var lumaNN = lumaNv + lumaM;
    let lumaSS = lumaSv + lumaM;
    let pairN = abs(gradientN) >= abs(gradientS);
    let gradient = max(abs(gradientN), abs(gradientS));
    if (pairN) {
        lengthSign = -lengthSign;
    }
    let subpixC = clamp(abs(subpixB) * (1.0 / range), 0.0, 1.0);

    var posB = uv;
    let offNP = select(vec2f(0.0, rcp.y), vec2f(rcp.x, 0.0), horzSpan);
    if (horzSpan) {
        posB.y += lengthSign * 0.5;
    } else {
        posB.x += lengthSign * 0.5;
    }

    var offsets = array<f32, 8>(1.0, 1.5, 2.0, 2.0, 2.0, 2.0, 4.0, 12.0);
    var posN = posB - offNP * offsets[0];
    var posP = posB + offNP * offsets[0];
    var lumaEndN = fxaa_luma(fxaa_sample(posN));
    var lumaEndP = fxaa_luma(fxaa_sample(posP));
    if (!pairN) {
        lumaNN = lumaSS;
    }
    let gradientScaled = gradient * 0.25;
    let lumaMM = lumaM - lumaNN * 0.5;
    let subpixD = (-2.0) * subpixC + 3.0;
    let subpixE = subpixC * subpixC;
    let subpixF = subpixD * subpixE;
    let lumaMLTZero = lumaMM < 0.0;

    lumaEndN -= lumaNN * 0.5;
    lumaEndP -= lumaNN * 0.5;
    var doneN = abs(lumaEndN) >= gradientScaled;
    var doneP = abs(lumaEndP) >= gradientScaled;

    var i = 1u;
    loop {
        if ((doneN && doneP) || i >= 8u) {
            break;
        }
        let step = offsets[i];
        if (!doneN) {
            posN = posN - offNP * step;
            lumaEndN = fxaa_luma(fxaa_sample(posN)) - lumaNN * 0.5;
            doneN = abs(lumaEndN) >= gradientScaled;
        }
        if (!doneP) {
            posP = posP + offNP * step;
            lumaEndP = fxaa_luma(fxaa_sample(posP)) - lumaNN * 0.5;
            doneP = abs(lumaEndP) >= gradientScaled;
        }
        i = i + 1u;
    }

    let dstN = select(uv.y - posN.y, uv.x - posN.x, horzSpan);
    let dstP = select(posP.y - uv.y, posP.x - uv.x, horzSpan);
    let goodSpanN = (lumaEndN < 0.0) != lumaMLTZero;
    let spanLength = dstP + dstN;
    let goodSpanP = (lumaEndP < 0.0) != lumaMLTZero;
    if (spanLength <= 0.0) {
        return center;
    }
    let directionN = dstN < dstP;
    let dst = min(dstN, dstP);
    let goodSpan = select(goodSpanP, goodSpanN, directionN);
    let subpixG = subpixF * subpixF;
    let pixelOffset = (dst * (-1.0 / spanLength)) + 0.5;
    let subpixH = subpixG * params.fxaa_subpix;
    let pixelOffsetGood = select(0.0, pixelOffset, goodSpan);
    let pixelOffsetSubpix = max(pixelOffsetGood, subpixH);

    var posM = uv;
    if (horzSpan) {
        posM.y += pixelOffsetSubpix * lengthSign;
    } else {
        posM.x += pixelOffsetSubpix * lengthSign;
    }
    return fxaa_sample(posM);
}

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

    if (params.color_grade == 0.0
        && params.fxaa_enabled == 0.0
        && params.filmic_enabled == 0.0
        && params.colour_enabled == 0.0
        && params.sepia_enabled == 0.0
        && params.ripple_enabled == 0.0
        && params.mask_debug == 0.0) {
        return source;
    }

    let p = position.xy;
    let v = params.viewport;
    if (p.x < v.x || p.y < v.y || p.x >= v.x + v.z || p.y >= v.y + v.w) {
        return source;
    }

    let mask = textureLoad(t_mask, vec2i(p), 0).r;
    if (params.mask_debug != 0.0 && mask > 0.5) {
        return vec4f(1.0, 0.0, 1.0, source.a);
    }

    var color = source;
    if (params.fxaa_enabled != 0.0) {
        let dims = vec2f(textureDimensions(t_source));
        let blended = fxaa_pass(linear_to_srgb(color.rgb), p / dims);
        color = vec4f(srgb_to_linear(blended), color.a);
    }
    if (params.ripple_enabled != 0.0 && mask > 0.5) {
        // ReShade Displace_Map-style refraction, gated by the engine's mask.
        let dims = vec2f(textureDimensions(t_source));
        let w = 6.2831853 / max(params.ripple_wavelength, 1.0);
        let t = params.ripple_time * params.ripple_speed * 6.2831853;
        let offset = vec2f(
            sin(p.y * w + t) * params.ripple_amplitude,
            cos(p.x * w * 1.3 + t * 1.1) * params.ripple_amplitude,
        );
        color = vec4f(
            textureSampleLevel(t_source, s_linear, (p + offset) / dims, 0.0).rgb,
            color.a,
        );
    }

    if (params.color_grade != 0.0) {
        var rgb = color.rgb;
        let luma = dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
        rgb = mix(vec3f(luma), rgb, params.saturation);
        rgb = (rgb - vec3f(0.5)) * params.contrast + vec3f(0.5);
        rgb = rgb + vec3f(params.brightness);
        color = vec4f(clamp(rgb, vec3f(0.0), vec3f(1.0)), color.a);
    }

    if (params.filmic_enabled != 0.0
        || params.colour_enabled != 0.0
        || params.sepia_enabled != 0.0) {
        var gamma = linear_to_srgb(color.rgb);
        if (params.filmic_enabled != 0.0) {
            gamma = filmic_pass(gamma);
        }
        if (params.colour_enabled != 0.0) {
            gamma = colourfulness_pass(gamma);
        }
        if (params.sepia_enabled != 0.0) {
            gamma = sepia_pass(gamma);
        }
        color = vec4f(srgb_to_linear(gamma), color.a);
    }
    return color;
}
