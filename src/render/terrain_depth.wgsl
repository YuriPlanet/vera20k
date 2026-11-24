// Terrain edits (DrawIt 0071C304/0071C34E -> 004990E0/00497390) and read-only
// Bullet edits (DrawIt 00468090 -> 00494B60/00497FD0, 00492D20/00496820)
// against the live Depth32Float test instead of a snapshot of old Z. VERA
// scheduling only (terrain_draw.rs `draw_depth_run`).
//
// For a candidate in 0..=65535 the stored low16 word is the candidate itself,
// and every native Z producer stores `stored_native_depth(word)`, so hardware
// Less on the stored depth is the signed comparison against the old word.
// Above 65535 that comparison never admits. Below zero it always admits and
// stores the wrapped word: the `_wrapped` entry points draw exactly those
// pixels with depth compare Always.
//
// A body replaces its pixel and restarts the pixel's shadow count; a shadow
// stores its Z and only counts (its colour write is masked). Shadow496250 then
// halves the packed word once per counted shadow (terrain_depth_resolve.wgsl).
// A read-only interval's pipelines mask the Z store, so every piece tests the
// Z the interval began with, as the original leaves do. An opaque SHP body may
// sit between these edits (zsprite_shader.wgsl `fs_counted`): it stores like a
// body here, and later pieces test the Z it stored, as in parent order.
struct DepthTestedOutput {
    @location(0) color: vec4f,
    @location(1) shadows: vec4f,
    @builtin(frag_depth) depth: f32,
};

fn depth_tested(input: VertexOutput, shadow: bool, wrapped: bool) -> DepthTestedOutput {
    let pixel = own_pixel(input);
    if pixel.candidate > 65535 || (pixel.candidate < 0) != wrapped { discard; }
    var output: DepthTestedOutput;
    output.depth = stored_native_depth(pixel.candidate);
    if shadow {
        output.shadows = vec4f(1.0 / 255.0);
    } else {
        output.color = vec4f(body_color(input, pixel.index), 1.0);
    }
    return output;
}
@fragment
fn fs_depth_body(input: VertexOutput) -> DepthTestedOutput { return depth_tested(input, false, false); }
@fragment
fn fs_depth_shadow(input: VertexOutput) -> DepthTestedOutput { return depth_tested(input, true, false); }
@fragment
fn fs_depth_body_wrapped(input: VertexOutput) -> DepthTestedOutput { return depth_tested(input, false, true); }
@fragment
fn fs_depth_shadow_wrapped(input: VertexOutput) -> DepthTestedOutput { return depth_tested(input, true, true); }

// A read-only body stores its packed word. A native palette body's colour is
// already one (`resolve_palette`); only a compatibility body needs packing.
fn packed_body(input: VertexOutput, body: DepthTestedOutput) -> DepthTestedOutput {
    var output = body;
    if input.palette_light.x >> 24u == 0u {
        output.color = vec4f(retail_linear(body_word(output.color.rgb)), 1.0);
    }
    return output;
}
// The entry points call `depth_tested` themselves. One more call level around
// its discard and `own_pixel`'s makes optimizing FXC stop with an internal
// error, so the DirectX 12 pipeline never builds.
@fragment
fn fs_depth_read_only_body(input: VertexOutput) -> DepthTestedOutput {
    return packed_body(input, depth_tested(input, false, false));
}
@fragment
fn fs_depth_read_only_body_wrapped(input: VertexOutput) -> DepthTestedOutput {
    return packed_body(input, depth_tested(input, false, true));
}
