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

    // `Pirate_Curve`: contrast curve strength, method (0 luma, 1 chroma,
    // 2 both) and the enable flag.
    curve_strength: f32,
    curve_method: f32,
    curve_enabled: f32,
    pad1_d: f32,

    // 1 tints masked texels magenta, to verify the engine's mask alignment;
    // then the `MultiLUT` atlas selector row, chroma mix and luma mix.
    mask_debug: f32,
    atlas_selector: f32,
    atlas_chroma: f32,
    atlas_luma: f32,

    // Gaussian blur: kernel step in frame pixels, blend over the original, and
    // the enable flag.
    blur_radius: f32,
    blur_strength: f32,
    blur_enabled: f32,
    sharpen_strength: f32,

    // Unsharp mask: cap on the per-channel delta, enable flag; then the
    // contrast-adaptive `SuperResolution` gain and its enable flag.
    sharpen_clamp: f32,
    sharpen_enabled: f32,
    sr_sharpness: f32,
    sr_enabled: f32,

    // Depth buffer texels per frame pixel, then the focus band.
    depth_scale_x: f32,
    depth_scale_y: f32,
    dof_focus: f32,
    dof_range: f32,

    // Focus blur radius and enable flag, then the AO radius and strength.
    dof_strength: f32,
    dof_enabled: f32,
    ssao_radius: f32,
    ssao_intensity: f32,

    // Native-Z gap an occluder must clear, then the AO enable flag, the
    // `MultiLUT` atlas enable flag and padding.
    ssao_bias: f32,
    ssao_enabled: f32,
    atlas_enabled: f32,
    pad3_c: f32,

    // Tone curve lane (0 Reinhard, 1 Hable, 2 ACES), exposure, enable flag,
    // then the first LUT mix strength.
    tonemap_operator: f32,
    tonemap_exposure: f32,
    tonemap_enabled: f32,
    lut_strength: f32,

    // Second and third LUT mix strengths (0 skips that table), then the LUT
    // enable flag and the SMAA enable flag.
    lut_strength2: f32,
    lut_strength3: f32,
    lut_enabled: f32,
    smaa_enabled: f32,

    // SMAA edge threshold and search steps (mirrored into the SMAA pass
    // uniform), then padding.
    smaa_threshold: f32,
    smaa_search: f32,
    pad5_a: f32,
    pad5_b: f32,

    // Deband: search radius in pixels, luma threshold, dither grain and the
    // enable flag.
    deband_radius: f32,
    deband_threshold: f32,
    deband_grain: f32,
    deband_enabled: f32,

    // Clarity: wide-radius local contrast, its strength and the enable flag.
    clarity_radius: f32,
    clarity_strength: f32,
    clarity_enabled: f32,
    pad6_a: f32,

    // Vibrance amount and enable flag.
    vibrance_amount: f32,
    vibrance_enabled: f32,
    pad6_b: f32,
    pad6_c: f32,

    // SuperResolution lane: 0 is the CAS-style arm, 1 the RCAS-style arm.
    sr_mode: f32,
    pad6_d: f32,
    pad6_e: f32,
    pad6_f: f32,
};

@group(0) @binding(0) var t_source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var s_linear: sampler;
// Engine-built water mask: 1 where the cell's land type is Water.
@group(0) @binding(3) var t_mask: texture_2d<f32>;
// Battlefield Z attachment: native row/height sort key, 1.0 empty.
@group(0) @binding(4) var t_depth: texture_depth_2d;
// 3D colour tables (`.cube`); never sampled unless a strength is positive.
@group(0) @binding(5) var t_lut: texture_3d<f32>;
// SMAA blend weights: R = left boundary, G = top boundary, both 0..0.5.
@group(0) @binding(6) var t_smaa: texture_2d<f32>;
// The second and third MultiLUT tables.
@group(0) @binding(7) var t_lut2: texture_3d<f32>;
@group(0) @binding(8) var t_lut3: texture_3d<f32>;
// `MultiLUT.fx`'s atlas sheet: 32x32 tiles, 32 across, 17 stacked LUTs.
@group(0) @binding(9) var t_atlas: texture_2d<f32>;

// ReShade `FilmicPass.fx` luminance coefficients.
const FILMIC_LUMA = vec3f(0.212656, 0.715158, 0.072186);
// `Pirate_Curve`'s "Luma" mode weights.
const CURVE_LUMA = vec3f(0.21, 0.72, 0.07);
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

// Port of ReShade `Pirate_Curve.fx`: split the colour into luma and chroma,
// raise one or both by `1 + strength`, recombine. Method 2 raises both with
// the same exponent, which cancels back to a per-channel gamma.
fn curve_pass(c: vec3f) -> vec3f {
    let strength = params.curve_strength;
    let exponent = 1.0 + strength;
    let luma = dot(c, CURVE_LUMA);
    if (params.curve_method < 0.5) {
        // Luma only: `w^k * (rgb / w)` is `rgb * w^(k-1)`.
        if (luma < 1e-4) {
            return c;
        }
        return c * pow(luma, strength);
    }
    if (params.curve_method < 1.5) {
        // Chroma only: `w * (rgb / w)^k`.
        if (luma < 1e-4) {
            return c;
        }
        return luma * pow(max(c / luma, vec3f(0.0)), vec3f(exponent));
    }
    return pow(max(c, vec3f(0.0)), vec3f(exponent));
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

// One 3x3 Gaussian (1 2 1 / 2 4 2 / 1 2 1) at `step` texels; the sampler
// interpolates inside its ring, so `step` scales the blur without more taps.
fn blur3(uv: vec2f, step: vec2f) -> vec3f {
    let c = textureSampleLevel(t_source, s_linear, uv, 0.0).rgb;
    let n = textureSampleLevel(t_source, s_linear, uv + vec2f(0.0, -step.y), 0.0).rgb;
    let s = textureSampleLevel(t_source, s_linear, uv + vec2f(0.0, step.y), 0.0).rgb;
    let e = textureSampleLevel(t_source, s_linear, uv + vec2f(step.x, 0.0), 0.0).rgb;
    let w = textureSampleLevel(t_source, s_linear, uv + vec2f(-step.x, 0.0), 0.0).rgb;
    let ne = textureSampleLevel(t_source, s_linear, uv + vec2f(step.x, -step.y), 0.0).rgb;
    let nw = textureSampleLevel(t_source, s_linear, uv + vec2f(-step.x, -step.y), 0.0).rgb;
    let se = textureSampleLevel(t_source, s_linear, uv + vec2f(step.x, step.y), 0.0).rgb;
    let sw = textureSampleLevel(t_source, s_linear, uv + vec2f(-step.x, step.y), 0.0).rgb;
    return (c * 4.0 + (n + s + e + w) * 2.0 + (ne + nw + se + sw)) * (1.0 / 16.0);
}

// Two rings: the configured radius, and twice it, so the falloff stays smooth
// instead of showing the kernel's square footprint.
fn blur_pass(uv: vec2f) -> vec3f {
    let dims = vec2f(textureDimensions(t_source));
    let rcp = vec2f(1.0 / dims.x, 1.0 / dims.y);
    let step = rcp * max(params.blur_radius, 0.0);
    return blur3(uv, step) * 0.65 + blur3(uv, step * 2.0) * 0.35;
}

// Neighbourhood average in gamma space, the space the sharpen shaders author.
fn cross_average(uv: vec2f, rcp: vec2f) -> vec3f {
    let n = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + vec2f(0.0, -rcp.y), 0.0).rgb);
    let s = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + vec2f(0.0, rcp.y), 0.0).rgb);
    let e = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + vec2f(rcp.x, 0.0), 0.0).rgb);
    let w = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + vec2f(-rcp.x, 0.0), 0.0).rgb);
    return (n + s + e + w) * 0.25;
}

// Unsharp mask in the style of ReShade `LumaSharpen.fx`: the difference from
// the neighbourhood average, capped at `sharpen_clamp` so edges cannot ring.
fn sharpen_pass(c: vec3f, uv: vec2f) -> vec3f {
    let dims = vec2f(textureDimensions(t_source));
    let rcp = vec2f(1.0 / dims.x, 1.0 / dims.y);
    let g = linear_to_srgb(c);
    let delta = g - cross_average(uv, rcp);
    let capped = clamp(delta, vec3f(-params.sharpen_clamp), vec3f(params.sharpen_clamp));
    return srgb_to_linear(g + capped * params.sharpen_strength);
}

// RCAS-style sharpening (`FidelityFX Super Resolution 1.0`): the same 5-tap
// laplacian as the CAS arm, but the result is clamped to the four neighbours'
// range so hard edges gain no halo. This is the sharpen FSR pairs with its
// upscale.
fn rcas_pass(c: vec3f, uv: vec2f) -> vec3f {
    let dims = vec2f(textureDimensions(t_source));
    let rcp = vec2f(1.0 / dims.x, 1.0 / dims.y);
    let n = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + vec2f(0.0, -rcp.y), 0.0).rgb);
    let s = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + vec2f(0.0, rcp.y), 0.0).rgb);
    let e = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + vec2f(rcp.x, 0.0), 0.0).rgb);
    let w = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + vec2f(-rcp.x, 0.0), 0.0).rgb);
    let g = linear_to_srgb(c);
    let avg = (n + s + e + w) * 0.25;
    let lo = min(min(n, s), min(e, w));
    let hi = max(max(n, s), max(e, w));
    // A small overshoot allowance keeps a reduced-scale sprite's edge crisp;
    // clamping to the exact neighbour range would flatten it instead.
    let margin = (hi - lo) * 0.12 + vec3f(0.004);
    let sharpened = clamp(g + (g - avg) * params.sr_sharpness, lo - margin, hi + margin);
    return srgb_to_linear(sharpened);
}

// Contrast-adaptive sharpening in the style of the ReShade `SuperResolution.fx`
// edge enhancement: the gain follows the local min/max range, so flat colour
// blocks keep their bytes while edges tighten.
fn super_resolution_pass(c: vec3f, uv: vec2f) -> vec3f {
    if (params.sr_mode > 0.5) {
        return rcas_pass(c, uv);
    }
    let dims = vec2f(textureDimensions(t_source));
    let rcp = vec2f(1.0 / dims.x, 1.0 / dims.y);
    let n = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + vec2f(0.0, -rcp.y), 0.0).rgb);
    let s = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + vec2f(0.0, rcp.y), 0.0).rgb);
    let e = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + vec2f(rcp.x, 0.0), 0.0).rgb);
    let w = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + vec2f(-rcp.x, 0.0), 0.0).rgb);
    let g = linear_to_srgb(c);
    let lo = min(min(min(n, s), min(e, w)), g);
    let hi = max(max(max(n, s), max(e, w)), g);
    // 8x the local range saturates quickly, which is the "sharp edges only"
    // gate; 0.02 keeps flat gradients from being amplified at all.
    let adapt = clamp((hi - lo - vec3f(0.02)) * 8.0, vec3f(0.0), vec3f(1.0));
    let delta = clamp(g - cross_average(uv, rcp), vec3f(-0.25), vec3f(0.25));
    return srgb_to_linear(g + delta * adapt * params.sr_sharpness);
}

// Native Z of the battlefield texel under frame pixel `p`. The depth target
// can be smaller than the frame (wheel zoom / reduced render scale), so the
// uniform carries the texel-per-pixel ratio.
fn depth_at(p: vec2f) -> f32 {
    let dims = vec2i(textureDimensions(t_depth));
    let coord = clamp(
        vec2i(p * vec2f(params.depth_scale_x, params.depth_scale_y)),
        vec2i(0),
        dims - vec2i(1),
    );
    return textureLoad(t_depth, coord, 0);
}

// Depth-contrast AO: a neighbour much nearer in native Z (smaller value)
// darkens this texel, which reads as contact shading behind objects. Empty
// sky (1.0) neither occludes nor is shaded.
fn ssao_pass(c: vec3f, p: vec2f) -> vec3f {
    let centre = depth_at(p);
    if (centre >= 1.0) {
        return c;
    }
    let radius = max(params.ssao_radius, 0.0);
    var occlusion = 0.0;
    for (var i = 0; i < 8; i += 1) {
        let angle = f32(i) * 0.785398;
        let d = depth_at(p + vec2f(cos(angle), sin(angle)) * radius);
        if (d < 1.0 && d < centre - params.ssao_bias) {
            occlusion += 1.0;
        }
    }
    for (var i = 0; i < 4; i += 1) {
        let angle = f32(i) * 1.570796 + 0.785398;
        let d = depth_at(p + vec2f(cos(angle), sin(angle)) * (radius * 0.5));
        if (d < 1.0 && d < centre - params.ssao_bias) {
            occlusion += 0.5;
        }
    }
    // 8 full + 4 half samples = 10.
    let shade = clamp(occlusion / 10.0 * params.ssao_intensity, 0.0, 1.0);
    return c * (1.0 - shade);
}

// Focus blur over the native-Z band: inside `focus ± range` stays sharp and
// the blur radius ramps with distance from the band.
fn depth_of_field_pass(c: vec3f, uv: vec2f, p: vec2f) -> vec3f {
    let depth = depth_at(p);
    let band = max(params.dof_range, 1e-4);
    let coc = clamp((abs(depth - params.dof_focus) - band) / band, 0.0, 1.0);
    if (coc <= 0.0) {
        return c;
    }
    let dims = vec2f(textureDimensions(t_source));
    let step = vec2f(1.0 / dims.x, 1.0 / dims.y) * (params.dof_strength * coc);
    return blur3(uv, step) * 0.65 + blur3(uv, step * 2.0) * 0.35;
}

// ACES (Narkowicz) tone curve.
fn aces(x: vec3f) -> vec3f {
    let a = 2.51;
    let b = 0.03;
    let c = 0.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + vec3f(b))) / (x * (c * x + vec3f(d)) + vec3f(e)), vec3f(0.0), vec3f(1.0));
}

// Uncharted2 / Hable curve.
fn hable_curve(x: vec3f) -> vec3f {
    let a = 0.15;
    let b = 0.50;
    let c = 0.10;
    let d = 0.20;
    let e = 0.02;
    let f = 0.30;
    return ((x * (a * x + vec3f(c * b)) + vec3f(d * e))
        / (x * (a * x + vec3f(b)) + vec3f(d * f))) - vec3f(e / f);
}

fn hable(x: vec3f) -> vec3f {
    let white = hable_curve(vec3f(11.2));
    return max(hable_curve(x * 2.0) / white, vec3f(0.0));
}

// Presentation tone curve; the frame is LDR, so this shapes mid-tones.
fn tonemap_pass(c: vec3f) -> vec3f {
    let x = max(c, vec3f(0.0)) * params.tonemap_exposure;
    if (params.tonemap_operator < 0.5) {
        return x / (vec3f(1.0) + x);
    }
    if (params.tonemap_operator < 1.5) {
        return clamp(hable(x), vec3f(0.0), vec3f(1.0));
    }
    return aces(x);
}

// Trilinear `.cube` sample coordinate in gamma space: the table's first texel
// centre maps to 0, the last to 1.
fn lut_coord(gamma: vec3f, size: f32) -> vec3f {
    return clamp((gamma * (size - 1.0) + 0.5) / size, vec3f(0.0), vec3f(1.0));
}

// `MultiLUT.fx`: the sheet stacks 17 LUTs of 32x32 tiles; each LUT is 32 tiles
// across, so the blue channel picks the tile column and the layer fraction
// interpolates across the two neighbouring slices.
const ATLAS_TILE = 32.0;
const ATLAS_TILES = 32.0;
const ATLAS_ROWS = 17.0;

fn atlas_lut_pass(c: vec3f) -> vec3f {
    let src_len = length(c);
    if (src_len < 1e-6) {
        return c;
    }
    let texel_y = 1.0 / ATLAS_TILE;
    let texel_x = texel_y / ATLAS_TILES;
    // `color * tile - color` leaves a hair of margin between tiles.
    var u = (c.x * ATLAS_TILE - c.x + 0.5) * texel_x;
    var v = (c.y * ATLAS_TILE - c.y + 0.5) * texel_y;
    let z = c.z * ATLAS_TILE - c.z;
    v = v / ATLAS_ROWS + params.atlas_selector / ATLAS_ROWS;
    let layer = fract(z);
    u = u + (z - layer) * texel_y;
    let lower = textureSampleLevel(t_atlas, s_linear, vec2f(u, v), 0.0).rgb;
    let upper = textureSampleLevel(t_atlas, s_linear, vec2f(u + texel_y, v), 0.0).rgb;
    let lut = mix(lower, upper, layer);
    if (length(lut) < 1e-6) {
        return c;
    }
    let chroma = mix(normalize(c), normalize(lut), clamp(params.atlas_chroma, 0.0, 1.0));
    let luma = mix(src_len, length(lut), clamp(params.atlas_luma, 0.0, 1.0));
    return chroma * luma;
}

// SMAA 1x neighborhood blending, ported from `SMAA.hlsli`. The weight map's
// R/G are the lateral weights, B/A the vertical ones; this picks the stronger
// axis and blends with the two texels the weights point at.
fn smaa_pass(color: vec3f, pi: vec2i) -> vec3f {
    let dims = vec2f(textureDimensions(t_source));
    let texel = 1.0 / dims;
    let uv = (vec2f(pi) + vec2f(0.5)) * texel;
    let right_w = textureSampleLevel(t_smaa, s_linear, uv + vec2f(texel.x, 0.0), 0.0).a;
    let top_w = textureSampleLevel(t_smaa, s_linear, uv + vec2f(0.0, texel.y), 0.0).g;
    let here = textureSampleLevel(t_smaa, s_linear, uv, 0.0);
    let a = vec4f(right_w, top_w, here.z, here.x);
    if (a.x + a.y + a.z + a.w < 1e-5) {
        return color;
    }
    let horizontal = max(a.x, a.z) > max(a.y, a.w);
    var offset = vec4f(0.0, a.y, 0.0, a.w);
    var weight = vec2f(a.y, a.w);
    if (horizontal) {
        offset = vec4f(a.x, 0.0, a.z, 0.0);
        weight = vec2f(a.x, a.z);
    }
    let total = weight.x + weight.y;
    if (total < 1e-5) {
        return color;
    }
    weight /= total;
    let ga = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + offset.xy * texel, 0.0).rgb);
    let gb = linear_to_srgb(textureSampleLevel(t_source, s_linear, uv + offset.zw * texel, 0.0).rgb);
    return srgb_to_linear(weight.x * ga + weight.y * gb);
}

// A cheap per-pixel hash: the deband dither must not repeat in a visible
// pattern, and the texture has no noise table bound.
fn pixel_hash(p: vec2i) -> f32 {
    var h = u32(p.x) * 374761393u + u32(p.y) * 668265263u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 0xFFFFu) / 65535.0;
}

// Deband in the spirit of ReShade `Deband.fx`: average the rotated neighbours
// whose luma sits inside the threshold, which smooths the 8-bit steps inside
// gradients while leaving edges alone, then dither the result so the steps the
// quantizer would recreate never line up.
fn deband_pass(c: vec3f, uv: vec2f, pi: vec2i) -> vec3f {
    let dims = vec2f(textureDimensions(t_source));
    let g = linear_to_srgb(c);
    let l = dot(g, vec3f(0.299, 0.587, 0.114));
    let threshold = max(params.deband_threshold, 0.0);
    let radius = max(params.deband_radius, 0.5);
    // Per-pixel rotation keeps the eight taps from forming a fixed lattice.
    let rotation = pixel_hash(pi) * 6.2831853;
    var sum = g;
    var count = 1.0;
    var spread = 0.0;
    var lowest = l;
    var highest = l;
    for (var i = 0; i < 8; i += 1) {
        let angle = rotation + f32(i) * 0.7853982;
        let sample = linear_to_srgb(
            textureSampleLevel(t_source, s_linear, uv + vec2f(cos(angle), sin(angle)) * radius / dims, 0.0).rgb,
        );
        let sample_l = dot(sample, vec3f(0.299, 0.587, 0.114));
        lowest = min(lowest, sample_l);
        highest = max(highest, sample_l);
        if (abs(sample_l - l) <= threshold) {
            sum += sample;
            count += 1.0;
        }
    }
    spread = highest - lowest;
    let smoothed = sum / count;
    // Dither only where the neighbourhood actually varies: a flat colour must
    // stay a byte copy, while a 1-bit band inside a gradient gains noise.
    let dither_scale = select(0.0, clamp(spread / max(threshold, 1e-5), 0.0, 1.0), spread > 0.002);
    let dither = (pixel_hash(pi + vec2i(37, 17)) - 0.5)
        * threshold
        * max(params.deband_grain, 0.0)
        * dither_scale;
    return srgb_to_linear(clamp(smoothed + vec3f(dither), vec3f(0.0), vec3f(1.0)));
}

// Clarity: local contrast at a wide radius, so texture and unit silhouettes
// read stronger without the tight halos a small unsharp mask leaves.
fn clarity_pass(c: vec3f, uv: vec2f) -> vec3f {
    let dims = vec2f(textureDimensions(t_source));
    let step = max(params.clarity_radius, 0.5) / dims;
    let g = linear_to_srgb(c);
    let delta = g - linear_to_srgb(blur3(uv, step));
    return srgb_to_linear(clamp(g + delta * params.clarity_strength, vec3f(0.0), vec3f(1.0)));
}

// Vibrance: scales chroma by how unsaturated the texel already is, so muted
// colours gain life while saturated ones stay put.
fn vibrance_pass(c: vec3f) -> vec3f {
    let g = linear_to_srgb(c);
    let l = dot(g, vec3f(0.299, 0.587, 0.114));
    let top = max(g.r, max(g.g, g.b));
    let bottom = min(g.r, min(g.g, g.b));
    let saturation = select(0.0, (top - bottom) / max(top, 1e-4), top > 1e-4);
    let gain = 1.0 + params.vibrance_amount * (1.0 - clamp(saturation, 0.0, 1.0));
    let out = vec3f(l) + (g - vec3f(l)) * gain;
    return srgb_to_linear(clamp(out, vec3f(0.0), vec3f(1.0)));
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
        && params.blur_enabled == 0.0
        && params.sharpen_enabled == 0.0
        && params.sr_enabled == 0.0
        && params.dof_enabled == 0.0
        && params.ssao_enabled == 0.0
        && params.tonemap_enabled == 0.0
        && params.lut_enabled == 0.0
        && params.smaa_enabled == 0.0
        && params.deband_enabled == 0.0
        && params.clarity_enabled == 0.0
        && params.vibrance_enabled == 0.0
        && params.curve_enabled == 0.0
        && params.atlas_enabled == 0.0
        && params.mask_debug == 0.0) {
        return source;
    }

    let p = position.xy;
    let pi = vec2i(p);
    let v = params.viewport;
    if (p.x < v.x || p.y < v.y || p.x >= v.x + v.z || p.y >= v.y + v.w) {
        return source;
    }

    let mask = textureLoad(t_mask, vec2i(p), 0).r;
    if (params.mask_debug != 0.0 && mask > 0.5) {
        return vec4f(1.0, 0.0, 1.0, source.a);
    }

    var color = source;
    let dims = vec2f(textureDimensions(t_source));
    let uv = p / dims;
    if (params.blur_enabled != 0.0) {
        color = vec4f(
            mix(color.rgb, blur_pass(uv), clamp(params.blur_strength, 0.0, 1.0)),
            color.a,
        );
    }
    if (params.fxaa_enabled != 0.0) {
        let blended = fxaa_pass(linear_to_srgb(color.rgb), uv);
        color = vec4f(srgb_to_linear(blended), color.a);
    }
    if (params.smaa_enabled != 0.0) {
        color = vec4f(smaa_pass(color.rgb, pi), color.a);
    }
    if (params.deband_enabled != 0.0) {
        color = vec4f(deband_pass(color.rgb, uv, pi), color.a);
    }
    if (params.sharpen_enabled != 0.0) {
        color = vec4f(sharpen_pass(color.rgb, uv), color.a);
    }
    if (params.sr_enabled != 0.0) {
        color = vec4f(super_resolution_pass(color.rgb, uv), color.a);
    }
    if (params.clarity_enabled != 0.0) {
        color = vec4f(clarity_pass(color.rgb, uv), color.a);
    }
    if (params.ssao_enabled != 0.0) {
        color = vec4f(ssao_pass(color.rgb, p), color.a);
    }
    if (params.tonemap_enabled != 0.0) {
        color = vec4f(tonemap_pass(color.rgb), color.a);
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
        || params.sepia_enabled != 0.0
        || params.lut_enabled != 0.0
        || params.curve_enabled != 0.0
        || params.atlas_enabled != 0.0) {
        var gamma = linear_to_srgb(color.rgb);
        if (params.curve_enabled != 0.0) {
            gamma = curve_pass(gamma);
        }
        if (params.filmic_enabled != 0.0) {
            gamma = filmic_pass(gamma);
        }
        if (params.colour_enabled != 0.0) {
            gamma = colourfulness_pass(gamma);
        }
        if (params.sepia_enabled != 0.0) {
            gamma = sepia_pass(gamma);
        }
        if (params.lut_enabled != 0.0) {
            if (params.lut_strength > 0.0) {
                let size = f32(textureDimensions(t_lut).x);
                let looked_up = textureSampleLevel(t_lut, s_linear, lut_coord(gamma, size), 0.0).rgb;
                gamma = mix(gamma, looked_up, clamp(params.lut_strength, 0.0, 1.0));
            }
            if (params.lut_strength2 > 0.0) {
                let size = f32(textureDimensions(t_lut2).x);
                let looked_up = textureSampleLevel(t_lut2, s_linear, lut_coord(gamma, size), 0.0).rgb;
                gamma = mix(gamma, looked_up, clamp(params.lut_strength2, 0.0, 1.0));
            }
            if (params.lut_strength3 > 0.0) {
                let size = f32(textureDimensions(t_lut3).x);
                let looked_up = textureSampleLevel(t_lut3, s_linear, lut_coord(gamma, size), 0.0).rgb;
                gamma = mix(gamma, looked_up, clamp(params.lut_strength3, 0.0, 1.0));
            }
        }
        if (params.atlas_enabled != 0.0) {
            gamma = atlas_lut_pass(gamma);
        }
        color = vec4f(srgb_to_linear(gamma), color.a);
    }
    if (params.vibrance_enabled != 0.0) {
        color = vec4f(vibrance_pass(color.rgb), color.a);
    }
    if (params.dof_enabled != 0.0) {
        color = vec4f(depth_of_field_pass(color.rgb, uv, p), color.a);
    }
    return color;
}
