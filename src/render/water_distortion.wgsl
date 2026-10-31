// The water distortion field: a frame-sized RG16F buffer of sampling offsets.
//
// The engine's water mask says which texels are water (the visible cells whose
// land type is `LandType::Water`). Inside water this pass writes a smooth,
// slowly scrolling displacement field encoded around 0.5 — RG hold the offset
// in [-1, 1] scaled by 0.5 — and outside water it writes the neutral 0.5 so the
// post-process pass can sample the whole frame without a mask lookup of its
// own. This is the buffer TC2's `DISTORT` semantic texture represents; the
// displacement is engine-owned rather than guessed from colour or depth.
//
// The field is two octaves of smooth value noise, scrolled by a time-speed
// drift, so water moves as a broad swell instead of one analytic sine pattern
// (which reads as parallel stripes once the crest highlight lifts it).

struct DistortParams {
    // Noise cell size in frame pixels: the INI `WaterRipple.Wavelength`.
    scale: f32,
    // Scroll speed, in noise cells per second.
    speed: f32,
    // Seconds since presentation started.
    time: f32,
    pad: f32,
};

@group(0) @binding(0) var<uniform> params: DistortParams;
// Engine-built water mask: 1 where the cell's land type is Water.
@group(0) @binding(1) var t_mask: texture_2d<f32>;

fn hash2(p: vec2f) -> vec2f {
    return fract(sin(vec2f(
        dot(p, vec2f(127.1, 311.7)),
        dot(p, vec2f(269.5, 183.3)),
    )) * vec2f(43758.5453, 22578.1459));
}

// Smooth (cubic-weighted) value noise, one independent channel per component.
fn value_noise(p: vec2f) -> vec2f {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2f(1.0, 0.0));
    let c = hash2(i + vec2f(0.0, 1.0));
    let d = hash2(i + vec2f(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Three scrolling octaves in [-1, 1]. The fine octaves put most of the
// displacement detail at a few pixels, so shorelines and units in water ripple
// visibly; a single low-frequency octave only slides broad blobs, which reads
// as static on the flat-coloured water interior.
fn displacement(p: vec2f) -> vec2f {
    let scale = max(params.scale, 1.0);
    let q = p / scale;
    // Wrap the drift so the noise coordinates stay small: `time` is wall-clock
    // seconds, and f32 loses the sub-frame detail the ripple needs once it
    // grows large (a multi-day uptime clock would freeze the field).
    let drift = (params.time * params.speed) % 1024.0;
    let a = value_noise(q + vec2f(drift, drift * 0.6)) * 2.0 - vec2f(1.0);
    let b = value_noise(q * 2.1 - vec2f(drift * 1.7, drift * 0.9) + vec2f(11.0, 5.0)) * 2.0
        - vec2f(1.0);
    let c = value_noise(q * 4.3 + vec2f(drift * 2.4, -drift * 1.4) + vec2f(23.0, 19.0)) * 2.0
        - vec2f(1.0);
    return a * 0.55 + b * 0.28 + c * 0.17;
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let water = textureLoad(t_mask, vec2i(position.xy), 0).r;
    if (water <= 0.5) {
        return vec4f(0.5, 0.5, 0.0, 1.0);
    }
    let d = displacement(position.xy);
    return vec4f(0.5 + 0.5 * d.x, 0.5 + 0.5 * d.y, 0.0, 1.0);
}
