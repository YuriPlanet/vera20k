// Terrain DrawIt 0071C304/0071C34E and Bullet DrawIt 00468090. Shared palette,
// native row walker and SHP projection precede this source. Terrain stores Z;
// Bullet uses the same signed admission with pipeline depth writes disabled.
@group(2) @binding(0) var old_words: texture_2d<u32>;
@group(2) @binding(1) var old_depth: texture_2d<f32>;

struct TerrainOutput {
    @location(0) color: vec4f,
    @builtin(frag_depth) depth: f32,
};

struct AdmittedPixel { index: u32, candidate: i32 };

fn admitted_pixel(input: VertexOutput) -> AdmittedPixel {
    let index = textureLoad(source_indices, source_texel(input.uv), 0).r;
    // Native compressed zero runs advance both destinations without touching
    // either one. The source index owns this stencil, including black colors.
    if index == 0u { discard; }
    let height = max(i32(round(input.rect_top_height.y)), 1);
    let top = i32(round(input.rect_top_height.x));
    let row = clamp(i32(floor(input.world_pos.y)) - top, 0, height - 1);
    let candidate = native_row_z(input.z_gradient, top - i32(round(camera.camera_pos.y + camera.native_z_origin_y)),
        height, i32(round(input.z_adjust)), row);
    let p = vec2i(input.position.xy);
    let previous = decoded_native_z(textureLoad(old_depth, p, 0).r);
    // 004990E0 / 00497390 compare signed candidate against zero-extended old
    // u16 BEFORE the Terrain store truncates. Bullet 00494B60/00497FD0 and
    // 00492D20/00496820 use this same comparison but never store Z. A repeated
    // negative candidate can pass; repeated Bullet shadows darken again.
    if candidate >= previous { discard; }
    return AdmittedPixel(index, candidate);
}

fn body_color(input: VertexOutput, index: u32) -> vec3f {
    let color = source_color(source_texel(input.uv), index, input.source_palette);
    return resolve_palette(color.rgb, input.tint, input.palette_light, index, tactical_a_at(input.position.xy));
}

fn terrain_pixel(input: VertexOutput, shadow: bool) -> TerrainOutput {
    let admitted = admitted_pixel(input);
    let p = vec2i(input.position.xy);
    var output: TerrainOutput;
    output.depth = stored_native_depth(admitted.candidate);
    if shadow {
        let word = native_half_word(textureLoad(old_words, p, 0).r);
        let encoded = vec3f(f32(RETAIL_FIVE[(word >> 11u) & 31u]),
            f32(RETAIL_SIX[(word >> 5u) & 63u]), f32(RETAIL_FIVE[word & 31u])) / 255.0;
        output.color = vec4f(srgb_decode(encoded), 1.0);
    } else {
        output.color = vec4f(body_color(input, admitted.index), 1.0);
    }
    return output;
}
@fragment
fn fs_body(input: VertexOutput) -> TerrainOutput { return terrain_pixel(input, false); }
@fragment
fn fs_shadow(input: VertexOutput) -> TerrainOutput { return terrain_pixel(input, true); }
