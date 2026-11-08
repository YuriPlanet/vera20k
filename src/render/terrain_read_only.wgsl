// The WebGL2 fallback's read-only intervals (terrain_draw.rs
// `reduce_read_only`). Bullet leaves 00494B60/00497FD0 overwrite an admitted
// body pixel; 00492D20/00496820 halve an admitted shadow pixel. Neither changes
// Z. Consequently the final word depends only on the last admitted body and
// admitted shadows after it. Bodies draw in command order into an R32Uint
// target, so the last admitted body is the one stored; each later admitted
// shadow adds 1/255 to an R8Unorm count by additive blending.
@group(3) @binding(0) var read_only_bodies: texture_2d<u32>;

@fragment
fn fs_read_only_body(input: VertexOutput) -> @location(0) u32 {
    let admitted = admitted_pixel(input);
    return (input.command_ordinal << 16u) | body_word(body_color(input, admitted.index));
}

@fragment
fn fs_read_only_shadow(input: VertexOutput) -> @location(0) vec4f {
    let admitted = admitted_pixel(input);
    let last_body = textureLoad(read_only_bodies, vec2i(input.position.xy), 0).r >> 16u;
    if input.command_ordinal <= last_body { discard; }
    return vec4f(1.0 / 255.0);
}
