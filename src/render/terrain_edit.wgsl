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

// Native Z of the row `world_y` falls in, for a blit whose top row and height
// are `top_height` (world pixels), walked with `z_gradient` and `z_adjust`.
// `candidate_range` in terrain_draw.rs bounds this walk on the CPU.
fn edit_z(world_y: f32, top_height: vec2f, z_adjust: f32, z_gradient: u32) -> i32 {
    let height = max(i32(round(top_height.y)), 1);
    let top = i32(round(top_height.x));
    let row = clamp(i32(floor(world_y)) - top, 0, height - 1);
    return native_row_z(z_gradient, top - i32(round(camera.camera_pos.y + camera.native_z_origin_y)),
        height, i32(round(z_adjust)), row);
}

fn own_pixel(input: VertexOutput) -> AdmittedPixel {
    let index = textureLoad(source_indices, source_texel(input.uv), 0).r;
    // Native compressed zero runs advance both destinations without touching
    // either one. The source index owns this stencil, including black colors.
    if index == 0u { discard; }
    return AdmittedPixel(index,
        edit_z(input.world_pos.y, input.rect_top_height, input.z_adjust, input.z_gradient));
}

fn snapshot_z(input: VertexOutput) -> i32 {
    return decoded_native_z(textureLoad(old_depth, vec2i(input.position.xy), 0).r);
}

fn admit(own: AdmittedPixel, previous: i32) -> AdmittedPixel {
    // 004990E0 / 00497390 compare signed candidate against zero-extended old
    // u16 BEFORE the Terrain store truncates. Bullet 00494B60/00497FD0 and
    // 00492D20/00496820 use this same comparison but never store Z. A repeated
    // negative candidate can pass; repeated Bullet shadows darken again.
    if own.candidate >= previous { discard; }
    return own;
}

fn admitted_pixel(input: VertexOutput) -> AdmittedPixel {
    return admit(own_pixel(input), snapshot_z(input));
}

fn body_color(input: VertexOutput, index: u32) -> vec3f {
    let color = source_color(source_texel(input.uv), index, input.source_palette);
    return resolve_palette(color.rgb, input.tint, input.palette_light, index, tactical_a_at(input.position.xy));
}

// The RGB565 word a read-only body leaf (00494B60/00497FD0) stores for `color`,
// the bound native Convert/LightConvert result.
fn body_word(color: vec3f) -> u32 {
    return native_rgb565_word(vec3u(round(clamp(srgb_encode(color), vec3f(0.0), vec3f(1.0)) * 255.0)));
}

// One window of a read-write wave in draw order (VERA scheduling only,
// `TerrainDrawRenderer::draw_wave`). Each edit first replays, at its own
// pixel, the earlier edits of its window it overlaps, so overlapping edits
// share one snapshot and one pass and still compose as one pass each would.
struct EditRecord {
    // Instance position and size, world pixels.
    quad: vec4f,
    // Instance uv_origin and uv_size.
    uv: vec4f,
    // The edit's scissor: x, y, width, height.
    rect: vec4u,
    light: vec4u,
    // z_adjust bits, z_gradient, source_palette, 1 for a shadow.
    params: vec4u,
    // A bit per earlier edit of the window this one replays.
    earlier: vec4u,
};
@group(3) @binding(1) var<uniform> edits: array<EditRecord, 64>;

struct ReplayedPixel { index: u32, texel: vec2i, candidate: i32 };

// What `edit` draws at the pixel centre `position`; index 0 where it draws
// nothing. Only quads on whole pixels at zoom 1 with a texel per pixel are
// replayed (`replays` in terrain_draw.rs): every centre lies half a texel and
// half a row inside them, so these equal the edit's own interpolated values.
fn replayed_pixel(edit: EditRecord, position: vec2f) -> ReplayedPixel {
    var none: ReplayedPixel;
    let p = vec2u(position);
    if any(p < edit.rect.xy) || any(p >= edit.rect.xy + edit.rect.zw) { return none; }
    let local = position - (edit.quad.xy - camera.camera_pos);
    if any(local < vec2f(0.0)) || any(local >= edit.quad.zw) { return none; }
    let t = local / edit.quad.zw;
    let texel = source_texel(edit.uv.xy + t * edit.uv.zw);
    let world_y = edit.quad.y + t.y * edit.quad.w;
    return ReplayedPixel(textureLoad(source_indices, texel, 0).r, texel,
        edit_z(world_y, edit.quad.yw, bitcast<f32>(edit.params.x), edit.params.y));
}

// The pixel as the earlier edits of this window left it: its Z, the last
// admitted body (slot + 1; 0 keeps the snapshot word) and the shadows
// admitted after that body.
struct Replayed { z: i32, body: u32, halves: u32 };
fn replay_earlier(input: VertexOutput) -> Replayed {
    var state = Replayed(snapshot_z(input), 0u, 0u);
    let slot = input.command_ordinal - 1u;
    let earlier = edits[slot].earlier.xy;
    for (var j = 0u; j < slot; j++) {
        if ((select(earlier.x, earlier.y, j >= 32u) >> (j % 32u)) & 1u) == 0u { continue; }
        let edit = edits[j];
        let pixel = replayed_pixel(edit, input.position.xy);
        if pixel.index == 0u || pixel.candidate >= state.z { continue; }
        // The store keeps the low 16 bits, which the next edit reads back.
        state.z = i32(bitcast<u32>(pixel.candidate) & 65535u);
        if edit.params.w == 0u {
            state.body = j + 1u;
            state.halves = 0u;
        } else {
            state.halves += 1u;
        }
    }
    return state;
}

fn terrain_pixel(input: VertexOutput, shadow: bool) -> TerrainOutput {
    let own = own_pixel(input);
    let state = replay_earlier(input);
    let admitted = admit(own, state.z);
    var output: TerrainOutput;
    output.depth = stored_native_depth(admitted.candidate);
    if shadow {
        var word = textureLoad(old_words, vec2i(input.position.xy), 0).r;
        if state.body != 0u {
            // A body a shadow replays is a native palette draw. Every presented
            // channel packs back to its own word, so the word it stores reads
            // back unchanged, as each halved word does.
            let edit = edits[state.body - 1u];
            let pixel = replayed_pixel(edit, input.position.xy);
            let color = source_color(pixel.texel, pixel.index, edit.params.z);
            word = native_light_word(color.rgb, edit.light, pixel.index,
                tactical_a_at(input.position.xy));
        }
        for (var i = 0u; i <= state.halves; i++) { word = native_half_word(word); }
        output.color = vec4f(retail_linear(word), 1.0);
    } else {
        output.color = vec4f(body_color(input, admitted.index), 1.0);
    }
    return output;
}
@fragment
fn fs_body(input: VertexOutput) -> TerrainOutput { return terrain_pixel(input, false); }
@fragment
fn fs_shadow(input: VertexOutput) -> TerrainOutput { return terrain_pixel(input, true); }
