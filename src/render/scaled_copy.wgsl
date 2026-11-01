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
    fsr: f32,        // 1 runs the FSR1 EASU reconstruction
    pad0: f32,
    pad1: f32,
    pad2: f32,
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

// Catmull-Rom kernel: a bicubic reconstruction of the source on upscale.
// It is sharper than a bilinear seam blend and, unlike nearest blocks, keeps
// diagonals continuous — the reconstruction the reduced render scale uses.
fn cubic(x: f32) -> f32 {
    let a = abs(x);
    let a2 = a * a;
    let a3 = a2 * a;
    if a < 1.0 {
        return 1.5 * a3 - 2.5 * a2 + 1.0;
    }
    if a < 2.0 {
        return -0.5 * a3 + 2.5 * a2 - 4.0 * a + 2.0;
    }
    return 0.0;
}

// One target texel reconstructed from the 4x4 source neighbourhood.
fn bicubic(centre: vec2f) -> vec4f {
    // Source texel centres sit at i + 0.5.
    let p = centre - vec2f(0.5);
    let base = floor(p);
    let t = p - base;
    let at = vec2i(base);
    var sum = vec4f(0.0);
    for (var j = 0; j < 4; j += 1) {
        let wy = cubic(t.y - f32(j - 1));
        for (var i = 0; i < 4; i += 1) {
            let wx = cubic(t.x - f32(i - 1));
            sum += texel(at + vec2i(i - 1, j - 1)) * (wx * wy);
        }
    }
    return sum;
}

// ---------------------------------------------------------------------------
// FSR1 EASU (AMD FidelityFX), ported from `FSR_EASU.hlsl` in TC2's effect pack.
// The twelve taps are loaded at their integer positions around the sample
// point instead of through `Gather`, which removes the API-specific gather
// ordering. FidelityFX works on gamma-encoded colour, so each tap is encoded
// before the edge analysis and the result is decoded for the sRGB target.

fn easu_gamma(c: vec3f) -> vec3f {
    return pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.2));
}

fn easu_linear(c: vec3f) -> vec3f {
    return pow(max(c, vec3f(0.0)), vec3f(2.2));
}

// FSR's cheap luma: 0.5*B + (0.5*R + G), i.e. luma times two.
fn easu_luma(c: vec3f) -> f32 {
    return c.b * 0.5 + (c.r * 0.5 + c.g);
}

fn easu_load(base: vec2i, offset: vec2i) -> vec3f {
    let dims = vec2i(textureDimensions(t_source));
    return easu_gamma(textureLoad(t_source, clamp(base + offset, vec2i(0), dims - vec2i(1)), 0).rgb);
}

// Tap accumulation with the anisotropy and negative lobe FSR1 uses.
fn easu_tap(off: vec2f, dir: vec2f, len: vec2f, lob: f32, clp: f32, c: vec3f) -> vec4f {
    var v = vec2f(
        off.x * dir.x + off.y * dir.y,
        off.x * (-dir.y) + off.y * dir.x,
    );
    v *= len;
    let d2 = min(dot(v, v), clp);
    let base = 25.0 / 16.0 * (2.0 / 5.0 * d2 - 1.0) * (2.0 / 5.0 * d2 - 1.0)
        - (25.0 / 16.0 - 1.0);
    let window = (lob * d2 - 1.0) * (lob * d2 - 1.0);
    let w = base * window;
    return vec4f(c * w, w);
}

// One of the four bilinear quadrants' contribution to direction and length.
fn easu_set(
    pp: vec2f,
    biS: bool,
    biT: bool,
    biU: bool,
    biV: bool,
    lA: f32,
    lB: f32,
    lC: f32,
    lD: f32,
    lE: f32,
) -> vec3f {
    var w = 0.0;
    if (biS) {
        w = (1.0 - pp.x) * (1.0 - pp.y);
    }
    if (biT) {
        w = pp.x * (1.0 - pp.y);
    }
    if (biU) {
        w = (1.0 - pp.x) * pp.y;
    }
    if (biV) {
        w = pp.x * pp.y;
    }
    let dc = lD - lC;
    let cb = lC - lB;
    let len_x = max(abs(dc), abs(cb));
    let dir_x = lD - lB;
    var x = 0.0;
    if (len_x > 1e-6) {
        x = abs(dir_x) / len_x;
        x = clamp(x, 0.0, 1.0);
        x = x * x;
    }
    let ec = lE - lC;
    let ca = lC - lA;
    let len_y = max(abs(ec), abs(ca));
    let dir_y = lE - lA;
    var y = 0.0;
    if (len_y > 1e-6) {
        y = abs(dir_y) / len_y;
        y = clamp(y, 0.0, 1.0);
        y = y * y;
    }
    return vec3f(dir_x * w, dir_y * w, (x + y) * w);
}

fn easu_reconstruction(pp: vec2f) -> vec4f {
    let base = vec2i(floor(pp));
    let frac = pp - vec2f(base);
    //    b c
    //  e f g h
    //  i j k l
    //    n o
    let b = easu_load(base, vec2i(0, -1));
    let c = easu_load(base, vec2i(1, -1));
    let e = easu_load(base, vec2i(-1, 0));
    let f = easu_load(base, vec2i(0, 0));
    let g = easu_load(base, vec2i(1, 0));
    let h = easu_load(base, vec2i(2, 0));
    let i = easu_load(base, vec2i(-1, 1));
    let j = easu_load(base, vec2i(0, 1));
    let k = easu_load(base, vec2i(1, 1));
    let l = easu_load(base, vec2i(2, 1));
    let n = easu_load(base, vec2i(0, 2));
    let o = easu_load(base, vec2i(1, 2));

    var dir = vec2f(0.0);
    var len = 0.0;
    let s0 = easu_set(frac, true, false, false, false, easu_luma(b), easu_luma(e), easu_luma(f), easu_luma(g), easu_luma(j));
    let s1 = easu_set(frac, false, true, false, false, easu_luma(c), easu_luma(f), easu_luma(g), easu_luma(h), easu_luma(k));
    let s2 = easu_set(frac, false, false, true, false, easu_luma(f), easu_luma(i), easu_luma(j), easu_luma(k), easu_luma(n));
    let s3 = easu_set(frac, false, false, false, true, easu_luma(g), easu_luma(j), easu_luma(k), easu_luma(l), easu_luma(o));
    dir = s0.xy + s1.xy + s2.xy + s3.xy;
    len = s0.z + s1.z + s2.z + s3.z;

    var d = dir;
    let dir_r = dot(dir, dir);
    if (dir_r < 1.0 / 32768.0) {
        d = vec2f(1.0, 0.0);
    } else {
        d = dir * (1.0 / sqrt(dir_r));
    }
    var shaped = len * 0.5;
    shaped = shaped * shaped;
    // Stretch the kernel along a diagonal edge; squash it across one.
    let stretch = (d.x * d.x + d.y * d.y) / max(max(abs(d.x), abs(d.y)), 1e-5);
    let len2 = vec2f(1.0 + (stretch - 1.0) * shaped, 1.0 - 0.5 * shaped);
    let lob = 0.5 + (0.21 - 0.5) * shaped;
    let clp = 1.0 / lob;

    var acc = vec3f(0.0);
    var wsum = 0.0;
    let offsets = array<vec2f, 12>(
        vec2f(0.0, -1.0),
        vec2f(1.0, -1.0),
        vec2f(-1.0, 1.0),
        vec2f(0.0, 1.0),
        vec2f(0.0, 0.0),
        vec2f(-1.0, 0.0),
        vec2f(1.0, 1.0),
        vec2f(2.0, 1.0),
        vec2f(2.0, 0.0),
        vec2f(1.0, 0.0),
        vec2f(1.0, 2.0),
        vec2f(0.0, 2.0),
    );
    let colours = array<vec3f, 12>(b, c, i, j, f, e, k, l, h, g, o, n);
    for (var tap = 0; tap < 12; tap += 1) {
        let r = easu_tap(offsets[tap] - frac, d, len2, lob, clp, colours[tap]);
        acc += r.xyz;
        wsum += r.w;
    }
    // Dering to the four nearest source texels.
    let lo = min(min(f, g), min(j, k));
    let hi = max(max(f, g), max(j, k));
    let value = min(hi, max(lo, acc / max(wsum, 1e-5)));
    let dims = vec2i(textureDimensions(t_source));
    let alpha = textureLoad(t_source, clamp(base, vec2i(0), dims - vec2i(1)), 0).a;
    return vec4f(easu_linear(value), alpha);
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
    // Upscale: reconstruct with a bicubic instead of blending two texels, so a
    // reduced battlefield resolution comes back with continuous edges.
    if params.fsr != 0.0 && params.scale >= 1.0 {
        let pp = (floor(position.xy) - params.origin) / params.scale;
        return easu_reconstruction(pp);
    }
    return bicubic(centre);
}
