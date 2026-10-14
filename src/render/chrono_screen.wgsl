struct Params {
    width: u32, height: u32, mode: u32, weight: u32,
    amplitude: f32, pad: f32, white_width: u32, white_height: u32,
    origin_x: u32, origin_y: u32, pad2: vec2<u32>,
}
@group(0) @binding(0) var snapshot: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var<storage, read> trig: array<f32>;

@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
    return vec4(positions[index], 0.0, 1.0);
}

//4CB150/4CB1A0: original table-index conversion, not platform sin/cos.
fn native_trig(angle: f32) -> vec2<f32> {
    let raw = i32(angle * 2607.594482421875);
    var index = (raw / 2) % 8192;
    if (index < 0) { index += 8192; }
    var cosine_index = index + 2048;
    if ((raw & 1) != 0) {
        if (index < 8191) { index += 1; }
        if (cosine_index < 10239) { cosine_index += 1; }
    }
    return vec2(trig[u32(index)], trig[u32(cosine_index)]);
}

@fragment fn fragment(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(pos.xy) - vec2<i32>(i32(params.origin_x), i32(params.origin_y));
    if (pixel.x < 0 || u32(pixel.x) >= params.width || pixel.y < 0 || u32(pixel.y) >= params.height) { discard; }
    if (params.mode == 1u) {
        //53B560 FillRect886FA0 is the tactical rectangle, not the sidebar.
        if (u32(pixel.x) >= params.white_width || u32(pixel.y) >= params.white_height) { discard; }
        return vec4(1.0);
    }
    if (params.mode == 2u) { return textureLoad(snapshot, pixel, 0); }
    let rows = i32(params.height) - 32; //retail7F5BFC; command bar survives.
    if (pixel.y >= rows || rows <= 0) { discard; }
    let centre = vec2<f32>(f32(params.width / 2u), f32(rows / 2));
    let local = vec2<f32>(pixel) - centre;
    let radial = (1.0 - dot(local, local) / dot(centre, centre)) * 5.0;
    let coeff = native_trig(radial * radial * params.amplitude);
    //Native7C5F00 uses retail0E7F chop; i32 conversion truncates toward0.
    let sampled = clamp(vec2<i32>(vec2(
        coeff.x * local.y + coeff.y * local.x,
        coeff.y * local.y - coeff.x * local.x
    ) + centre), vec2(0), vec2(i32(params.width) - 1, rows - 1));
    let colour = textureLoad(snapshot, sampled, 0);
    let bytes = vec3<u32>(colour.rgb * 255.0 + vec3(0.5));
    var components = vec3<u32>(bytes.r >> 3u, bytes.g >> 2u, bytes.b >> 3u);
    let maximum = vec3<u32>(31u, 63u, 31u);
    components += ((maximum - components) * (params.weight & 65535u)) >> vec3<u32>(8u);
    return vec4(vec3<f32>(components) / vec3(31.0,63.0,31.0), 1.0);
}
