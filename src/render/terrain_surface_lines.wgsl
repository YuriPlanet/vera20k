// Original4BEAC0/4BDF00/4BFD30: ordered RGB565 operations, strict u16 Z.
// Signed shifts use WGSL arithmetic shift: https://www.w3.org/TR/WGSL/#bit-expr
@group(0) @binding(0) var old_words: texture_2d<u32>;
@group(0) @binding(1) var old_depth: texture_2d<f32>;
struct Operation { z: u32, strength: i32, rgb: u32, alpha: u32, mode: u32 };
@group(1) @binding(0) var<storage, read> operations: array<Operation>;
struct Output {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) span: vec2u,
};
@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, @location(0) rect: vec4f,
           @location(1) span: vec2u) -> Output {
    let corners = array<vec2f,6>(vec2f(0,0),vec2f(1,0),vec2f(0,1),
        vec2f(0,1),vec2f(1,0),vec2f(1,1));
    let p = rect.xy + corners[vertex] * rect.zw;
    var out: Output;
    out.position = vec4f(p,0,1);
    out.span = span;
    return out;
}
@fragment
fn fs_main(input: Output) -> @location(0) vec4f {
    let p = vec2i(input.position.xy);
    let old_z = u32(decoded_native_z(textureLoad(old_depth,p,0).r));
    var word = textureLoad(old_words,p,0).r;
    var touched = false;
    for(var i = input.span.x; i < input.span.x + input.span.y; i += 1u) {
        let op = operations[i];
        if op.z >= old_z || op.alpha == 0u { continue; }
        let dst = vec3i(i32((word >> 11u) & 31u) << 3u,
            i32((word >> 5u) & 63u) << 2u, i32(word & 31u) << 3u);
        let src = vec3i(i32(op.rgb & 255u),i32((op.rgb >> 8u) & 255u),i32((op.rgb >> 16u) & 255u));
        var lit: vec3i;
        if op.mode == 0u {
            let mixed = ((dst * (256 - op.strength)) >> vec3u(8))
                + ((src * op.strength) >> vec3u(8));
            lit = (mixed * i32(op.alpha)) >> vec3u(7);
        } else if op.mode == 1u {
            // DSurface4BDF00 -> RGB4BE9D0: additive saturation distributes
            // excess to channels below255 before the final cap and pack.
            lit = dst + ((src * i32(op.alpha)) >> vec3u(7));
            let full = lit >= vec3i(255);
            let count = select(0,1,full.r) + select(0,1,full.g) + select(0,1,full.b);
            var excess = max(lit.r-255,0) + max(lit.g-255,0) + max(lit.b-255,0);
            if count == 1 { excess = excess >> 1u; }
            if count > 0 && count < 3 {
                lit += select(vec3i(excess), vec3i(0), full);
            }
            lit = min(lit,vec3i(255));
        } else {
            // DSurface4BFD30: alpha127 copies the packed source unchanged.
            if op.alpha == 127u {
                word = op.rgb;
                touched = true;
                continue;
            }
            let packed = vec3i(i32((op.rgb >> 11u) & 31u) << 3u,
                i32((op.rgb >> 5u) & 63u) << 2u, i32(op.rgb & 31u) << 3u);
            lit = (packed * i32(op.alpha)) >> vec3u(7);
        }
        word = u32(((lit.r >> 3u) << 11u) | ((lit.g >> 2u) << 5u) | (lit.b >> 3u)) & 65535u;
        touched = true;
    }
    if !touched { discard; }
    let encoded = vec3f(f32(RETAIL_FIVE[(word >> 11u) & 31u]),
        f32(RETAIL_SIX[(word >> 5u) & 63u]),f32(RETAIL_FIVE[word & 31u])) / 255.0;
    return vec4f(srgb_decode(encoded),1);
}
