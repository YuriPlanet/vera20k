// Shared native palette conversion: 00556090 -> 007DE200 (active MMX565),
// row LUT 00420140. Sources and executable oracle are cited in palette_light.rs.
fn srgb_encode(c: vec3f) -> vec3f {
    return select(1.055 * pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055,
                  c * 12.92, c <= vec3f(0.0031308));
}
fn srgb_decode(c: vec3f) -> vec3f {
    return select(pow((c + 0.055) / 1.055, vec3f(2.4)), c / 12.92,
                  c <= vec3f(0.04045));
}
fn palette_light(rgb_linear: vec3f, tint: vec3f) -> vec3f {
    return srgb_decode(clamp(srgb_encode(rgb_linear) * tint, vec3f(0.0), vec3f(1.0)));
}
fn native_palette_word(rgb: vec3u, index: u32, light: vec4u, a: u32) -> u32 {
    if index == 0u { return 0u; }
    let n = light.x >> 24u;
    let brightness = u32(max(bitcast<i32>(light.w), 0));
    let q = min((min(brightness, 2000u) * 261u) >> 11u, 254u);
    let row = min(a * q * (n - 1u) / 32258u, n - 1u);
    var scale = light.xyz & vec3u(0x3ffffu);
    if n > 1u { scale = scale * (2u * row) / (n - 1u); }
    if light.y >> 31u != 0u && index >= 240u && index <= 254u {
        let d = min(max(i32(n * 30u / 200u) - 1, 0), i32((n - 1u) / 2u));
        var neutral = 65536u;
        if n > 1u && row <= u32(d) { neutral = row * 65536u / u32(d); }
        scale = vec3u(neutral);
    }
    var lit = min((rgb * (scale >> vec3u(4u))) >> vec3u(12u), vec3u(255u));
    if (light.y & 0x40000000u) != 0u { lit = min((rgb * scale) >> vec3u(16u), vec3u(255u)); }
    return ((lit.r >> 3u) << 11u) | ((lit.g >> 2u) << 5u) | (lit.b >> 3u);
}
// Original tinted copies494CD9/498293/4993C2. Translucent slots forward
// to plain leaves; callers select whether the opaque colour word applies.
fn native_colour_word(word: u32, index: u32, light: vec4u) -> u32 {
    if index == 0u { return word; }
    return word | (((light.x >> 18u) & 31u) << 11u) | (((light.y >> 18u) & 63u) << 5u)
        | ((light.z >> 18u) & 31u);
}
fn resolve_palette(rgb_linear: vec3f, tint: vec3f, light: vec4u, index: u32, a: u32) -> vec3f {
    if light.x >> 24u == 0u {
        // Precomposed RGBA, alpha and FX lack native palette/packed-composition
        // metadata. Retain their compatibility source shading, now at the
        // source store rather than multiplying previously drawn surface lines.
        // Surface/UI bindings supply neutral A. This is not native alpha parity.
        return palette_light(rgb_linear, tint) * (f32(a) / 127.0);
    }
    let rgb = vec3u(round(clamp(srgb_encode(rgb_linear), vec3f(0.0), vec3f(1.0)) * 255.0));
    // 493E52/494BDD/4991F4 read A only after source-hole/Z admission. A=0
    // still stores palette row zero; N1 and special indices keep their rules.
    var word = native_palette_word(rgb, index, light, a);
    word = native_colour_word(word, index, light);
    let encoded = vec3f(f32(RETAIL_FIVE[(word >> 11u) & 31u]),
                        f32(RETAIL_SIX[(word >> 5u) & 63u]), f32(RETAIL_FIVE[word & 31u])) / 255.0;
    return srgb_decode(encoded);
}

// This increment owns opaque, unmodified source pixels. Packed native alpha
// compositing and cloak/effect routing retain their compatibility paths.
fn opaque_palette(light: vec4u, alpha: f32, flags: u32) -> vec4u {
    if (alpha < 1.0 || flags != 0u) { return vec4u(0u); }
    return light;
}
