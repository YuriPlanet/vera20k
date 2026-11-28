// Per-pixel Z-depth shader for TMP terrain tiles.
//
// TMP_TileBlitter @ 0x00547CF0 adds its Z-data byte to the base.
// The ground row is canvas_top - (z_adjust + byte), where z_adjust
// carries draw_offset.y - tileH - 15*level. One native Z unit is one
// world row; low16 storage shares the sprite paths' attachment axis.

struct Camera {
    screen_size: vec2f,
    camera_pos: vec2f,
    zoom: f32,
    world_origin_y: f32,
    world_height: f32,
    pad1: f32,
    native_z_origin_y: f32,
    native_z_pan: f32,
};
@group(0) @binding(0) var<uniform> camera: Camera;

// The tile atlas (encoded colour), its sampler and its R8 depth plane.
// VERA-only, experimental (docs/zoom-one-pass.md): while the battlefield
// draws zoomed in with HD art on, the atlas's HD binding
// (`TileAtlas::bind_hd_twin`) binds its 2x twin at 0 (premultiplied, two
// levels) with a linear sampler; the depth plane stays the 1x one, so a
// colour page larger than the depth plane is the twin.
@group(1) @binding(0) var t_sprite: texture_2d<f32>;
@group(1) @binding(1) var s_sprite: sampler;
@group(1) @binding(2) var t_zdepth: texture_2d<f32>;

struct Instance {
    @location(0) position: vec2f,
    @location(1) size: vec2f,
    @location(2) uv_origin: vec2f,
    @location(3) uv_size: vec2f,
    @location(4) depth: f32,
    @location(5) tint: vec3f,
    @location(6) alpha: f32,
    @location(11) z_adjust: f32,
    @location(14) palette_light: vec4u,
};

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
    @location(1) tint: vec3f,
    @location(2) @interpolate(flat) canvas_top: f32,
    @location(3) @interpolate(flat) z_adjust: f32,
    @location(11) @interpolate(flat) palette_light: vec4u,
};

@vertex
fn vs_main(
    @builtin(vertex_index) idx: u32,
    instance: Instance,
) -> VertexOutput {
    var quad_pos = array<vec2f, 6>(
        vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0),
    );
    var quad_uv = array<vec2f, 6>(
        vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0),
    );

    let local: vec2f = quad_pos[idx];
    // Snap the corners to whole world pixels, then scale by the zoom: each
    // world pixel is one zoom-sized block (docs/zoom-one-pass.md), the same
    // rounding as the other tactical blitters; at zoom 1 the plain rounding.
    let pixel_pos: vec2f = world_block_corner(instance.position + local * instance.size
        - camera.camera_pos);

    let clip_x: f32 = (pixel_pos.x / camera.screen_size.x) * 2.0 - 1.0;
    let clip_y: f32 = 1.0 - (pixel_pos.y / camera.screen_size.y) * 2.0;

    var output: VertexOutput;
    // Set position.z to 0.5 (midpoint) — frag_depth overrides the actual depth.
    output.position = vec4f(clip_x, clip_y, 0.5, 1.0);
    output.uv = instance.uv_origin + quad_uv[idx] * instance.uv_size;
    output.tint = instance.tint;
    output.canvas_top = instance.position.y;
    output.z_adjust = instance.z_adjust;
    output.palette_light = instance.palette_light;
    return output;
}


// RA2_DEBUG_DEPTH_VIEW (camera.pad1 > 0.5): depth as grey, wrapping every
// 128 world rows, so depth ordering can be read off a screenshot.
fn debug_depth_color(depth: f32) -> vec4f {
    let rows: f32 = world_row_from_native_depth(depth,
        native_z_origin_row(camera.camera_pos.y + camera.native_z_origin_y, camera.native_z_pan));
    let g: f32 = fract(rows / 128.0);
    return vec4f(g, g, g, 1.0);
}

struct FragOutput {
    @location(0) color: vec4f,
    @builtin(frag_depth) depth: f32,
};


// Palette and stored-depth mechanisms are supplied by tactical_shader::source.

@fragment
fn fs_main(input: VertexOutput) -> FragOutput {
    // Derivatives first, in uniform control flow.
    let uv_dx = dpdx(input.uv);
    let uv_dy = dpdy(input.uv);
    let depth_size = textureDimensions(t_zdepth);
    let page_size = textureDimensions(t_sprite);
    var color: vec4f;
    if (page_size.x > depth_size.x) {
        // The HD twin, at the level whose texels match the target's pixels
        // (log2 of the twin texels a target pixel spans, clamped by the
        // sampler to its last level, one texel per tile pixel); less than
        // half covered is outside the tile.
        let dims = vec2f(page_size);
        let span = max(length(uv_dx * dims), length(uv_dy * dims));
        let lod = max(log2(max(span, 1e-6)), 0.0);
        let twin = textureSampleLevel(t_sprite, s_sprite, input.uv, lod);
        if (twin.a < 0.5) {
            discard;
        }
        color = vec4f(min(twin.rgb / twin.a, vec3f(1.0)), 1.0);
    } else {
        // Tile pages are Rgba8Unorm: this is already the encoded colour.
        color = textureSampleLevel(t_sprite, s_sprite, input.uv, 0.0);
    }
    if (color.a < 0.01) {
        discard;
    }

    // R8 depth atlas: byte 0..255 stored as 0..1, the texel a nearest
    // sample of `uv` reads.
    let depth_texel = vec2i(clamp(input.uv * vec2f(depth_size), vec2f(0.0), vec2f(depth_size) - 1.0));
    let z_byte: f32 = round(textureLoad(t_zdepth, depth_texel, 0).r * 255.0);
    let ground_row: f32 = input.canvas_top - (input.z_adjust + z_byte);
    let frag_depth: f32 = stored_native_depth(32768
        + native_z_origin_row(camera.camera_pos.y + camera.native_z_origin_y, camera.native_z_pan)
        - i32(round(ground_row)));

    var output: FragOutput;
    output.color = vec4f(resolve_palette(color.rgb, input.tint, opaque_palette(input.palette_light, color.a, 0u), 1u, tactical_a_at(input.position.xy)), color.a);
    output.depth = frag_depth;
    if (camera.pad1 > 0.5) {
        output.color = debug_depth_color(frag_depth);
    }
    return output;
}
