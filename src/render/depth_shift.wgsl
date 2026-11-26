// Moves a kept layer of stored native Z under a scrolled camera.
// Group 0: the kept depth, and the pixel offset and rows of the move.
struct Shift { offset: vec2i, rows: i32, pad: i32 };
@group(0) @binding(0) var kept: texture_depth_2d;
@group(0) @binding(1) var<uniform> shift: Shift;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let corner = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @builtin(frag_depth) f32 {
    let z = decoded_native_z(textureLoad(kept, vec2i(floor(position.xy)) + shift.offset, 0));
    // A pixel nothing wrote stays empty; only written words count rows.
    if z == 65535 {
        return 1.0;
    }
    return stored_native_depth(z + shift.rows);
}
