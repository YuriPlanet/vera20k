// Anime4K CNN passes, driven by the weight tables parsed from `A4K_*.hlsl`.
//
// Every convolution pass reads RGBA layers through `textureLoad` (the effect
// code samples whole texels) and writes one to three output layers. Layers
// live in two ping-pong texture arrays, so a pass reads the array the previous
// pass wrote and writes the other one; the final pass reads whichever array
// each of its skip-connection layers lives in.

struct Params {
    size: vec2u,      // convolution lattice (the reduced world size)
    in_count: u32,    // number of input layers
    taps: u32,        // 9 for 3x3, 1 for 1x1
    linear: u32,      // 1: no activation (the first pass)
    bias_base: u32,   // first bias lane, after the matrices
    targets: u32,     // output groups on the final pass
    _pad: u32,
    layers: array<u32, 16>, // (array << 16) | layer
};

@group(0) @binding(0) var tex_a: texture_2d_array<f32>;
@group(0) @binding(1) var tex_b: texture_2d_array<f32>;
@group(0) @binding(2) var tex_c: texture_2d_array<f32>;
@group(0) @binding(3) var samp: sampler;
@group(0) @binding(4) var<storage, read> params: Params;
@group(0) @binding(5) var<storage, read> weights: array<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn fetch(at: vec2i, slot: u32) -> vec4f {
    let group = params.layers[slot];
    let limit = vec2i(params.size) - vec2i(1);
    let coords = clamp(at, vec2i(0), limit);
    let layer = i32(group & 0xffffu);
    let array = group >> 16u;
    if (array == 0u) {
        return textureLoad(tex_a, coords, layer, 0);
    }
    if (array == 1u) {
        return textureLoad(tex_b, coords, layer, 0);
    }
    return textureLoad(tex_c, coords, layer, 0);
}

// One matrix column set: lane `c` of the output is input dot weights[c*4..].
fn column(base: u32, v: vec4f) -> vec4f {
    return vec4f(
        dot(vec4f(weights[base], weights[base + 1u], weights[base + 2u], weights[base + 3u]), v),
        dot(
            vec4f(
                weights[base + 4u],
                weights[base + 5u],
                weights[base + 6u],
                weights[base + 7u],
            ),
            v,
        ),
        dot(
            vec4f(
                weights[base + 8u],
                weights[base + 9u],
                weights[base + 10u],
                weights[base + 11u],
            ),
            v,
        ),
        dot(
            vec4f(
                weights[base + 12u],
                weights[base + 13u],
                weights[base + 14u],
                weights[base + 15u],
            ),
            v,
        ),
    );
}

fn tap_offset(tap: u32) -> vec2i {
    if (params.taps <= 1u) {
        return vec2i(0, 0);
    }
    return vec2i(i32(tap % 3u) - 1, i32(tap / 3u) - 1);
}

fn conv(group: u32, at: vec2i) -> vec4f {
    let base = params.bias_base + group * 4u;
    var sum = vec4f(
        weights[base],
        weights[base + 1u],
        weights[base + 2u],
        weights[base + 3u],
    );
    for (var tap = 0u; tap < params.taps; tap += 1u) {
        let sampled = at + tap_offset(tap);
        for (var slot = 0u; slot < params.in_count; slot += 1u) {
            let v = fetch(sampled, slot);
            let w = ((group * params.in_count + slot) * params.taps + tap) * 32u;
            if (params.linear != 0u) {
                sum += column(w, v);
            } else {
                sum += column(w, max(v, vec4f(0.0)));
                sum += column(w + 16u, max(-v, vec4f(0.0)));
            }
        }
    }
    return sum;
}

struct One {
    @location(0) first: vec4f,
};

struct Two {
    @location(0) first: vec4f,
    @location(1) second: vec4f,
};

struct Three {
    @location(0) first: vec4f,
    @location(1) second: vec4f,
    @location(2) third: vec4f,
};

@fragment
fn fs_conv1(@builtin(position) position: vec4f) -> One {
    return One(conv(0u, vec2i(floor(position.xy))));
}

@fragment
fn fs_conv2(@builtin(position) position: vec4f) -> Two {
    let at = vec2i(floor(position.xy));
    return Two(conv(0u, at), conv(1u, at));
}

@fragment
fn fs_conv3(@builtin(position) position: vec4f) -> Three {
    let at = vec2i(floor(position.xy));
    return Three(conv(0u, at), conv(1u, at), conv(2u, at));
}

// The 2x2 corner an output pixel belongs to: x for (0, 0), y for (1, 0),
// z for (0, 1), w for (1, 1) — the order the effect code writes them.
fn corner(pixel: vec2u) -> u32 {
    let x = pixel.x & 1u;
    let y = pixel.y & 1u;
    if (x == 0u) {
        if (y == 0u) {
            return 0u;
        }
        return 3u;
    }
    if (y == 0u) {
        return 1u;
    }
    return 2u;
}

@fragment
fn fs_final(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let pixel = vec2u(floor(position.xy));
    let at = vec2i(i32(pixel.x / 2u), i32(pixel.y / 2u));
    let index = corner(pixel);
    var first = conv(0u, at);
    var second = first;
    var third = first;
    if (params.targets > 1u) {
        second = conv(1u, at);
        third = conv(2u, at);
    }
    var colour = vec3f(first[index]);
    if (params.targets > 1u) {
        colour = vec3f(first[index], second[index], third[index]);
    }
    // The reconstructed block rides on a bilinear sample of the source at the
    // output pixel centre, as the effect's last pass does.
    let uv = position.xy * 0.5 / vec2f(params.size);
    return vec4f(colour + textureSampleLevel(tex_a, samp, uv, 0, 0.0).rgb, 1.0);
}
