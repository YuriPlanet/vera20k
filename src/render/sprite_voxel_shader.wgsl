// Voxel sprite fragment shader.
//
// Atlas tiles store post-VPL, pre-house-remap palette indices (R8Uint).
// At fragment time:
//   byte = textureLoad(atlas, uv);
//   if (byte == 0) discard;
//   if (16 <= byte < 32) → rgb = house_ramp[house_idx][byte - 16]
//   else                 → rgb = palette[byte]
//   color = apply_fx(color, fx_flags, fx_params);
//   return palette_light(rgb, tint), alpha;  (tint applied in sRGB byte space)
//
// Bind groups:
//   group 0: camera uniform
//   group 1: atlas (R8Uint)
//   group 2: palette (Rgba8UnormSrgb) + house_ramp (Rgba8UnormSrgb) + sampler
//   (sRGB format → sampler returns linear RGB; the tint is applied after
//   re-encoding to sRGB, the space gamemd's LightConvert scales in)

struct Camera {
    screen_size: vec2f,
    camera_pos: vec2f,
    zoom: f32,
    // Depth axis: depth = 1 - (row - world_origin_y) / world_height.
    world_origin_y: f32,
    world_height: f32,
    pad1: f32,
    native_z_origin_y: f32,
    native_z_pad: f32,
};

@group(0) @binding(0) var<uniform> camera: Camera;

@group(1) @binding(0) var atlas: texture_2d<u32>;

@group(2) @binding(0) var palette: texture_2d<f32>;
@group(2) @binding(1) var house_ramp: texture_2d<f32>;
@group(2) @binding(2) var palette_sampler: sampler;

struct Instance {
    @location(0) position: vec2f,
    @location(1) size: vec2f,
    @location(2) uv_origin: vec2f,
    @location(3) uv_size: vec2f,
    @location(4) depth: f32,
    @location(5) tint: vec3f,
    @location(6) alpha: f32,
    @location(7) remap_row: u32,
    @location(8) fx_flags: u32,
    @location(9) fx_params: vec4f,
    @location(10) sinking_row: f32,
    @location(11) z_adjust: f32,
    @location(12) z_gradient: u32,
    // (top, height) of the composite blit rect this layer belongs to; zero
    // height means the layer's own quad. A turreted unit's hull, turret and
    // barrel are one native cache blit (`0x0073B140`), so they share one seed.
    @location(13) z_rect: vec2f,
    @location(14) palette_light: vec4u,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4f,
    @location(0) atlas_uv: vec2f,
    @location(1) tint: vec3f,
    @location(2) alpha: f32,
    @location(3) @interpolate(flat) remap_row: u32,
    @location(4) @interpolate(flat) fx_flags: u32,
    @location(5) fx_params: vec4f,
    @location(6) @interpolate(flat) sinking_row: f32,
    // World-pixel position of this fragment (unpadded quad).
    @location(7) world_pos: vec2f,
    // Blit rect top row and height in world pixels.
    @location(8) @interpolate(flat) rect_top_height: vec2f,
    @location(9) @interpolate(flat) z_adjust: f32,
    @location(10) @interpolate(flat) z_gradient: u32,
    @location(11) @interpolate(flat) palette_light: vec4u,
};

@vertex
fn vs_main(
    @builtin(vertex_index) idx: u32,
    instance: Instance,
) -> VertexOutput {
    // Quad vertices: 6 vertices forming 2 triangles (matches batch_shader.wgsl).
    var quad_pos = array<vec2f, 6>(
        vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0),
    );
    var quad_uv = array<vec2f, 6>(
        vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0),
    );

    let local: vec2f = quad_pos[idx];
    let is_zoomed: bool = abs(camera.zoom - 1.0) >= 0.001;
    let pad: f32 = select(0.0, 0.5 / camera.zoom, is_zoomed);
    let raw_pos: vec2f = (instance.position - vec2f(pad, pad)
        + local * (instance.size + vec2f(pad * 2.0, pad * 2.0))
        - camera.camera_pos) * camera.zoom;
    let pixel_pos: vec2f = select(raw_pos, floor(raw_pos + vec2f(0.5, 0.5)), !is_zoomed);

    // Convert pixel to clip space (matches batch_shader convention).
    let clip_x: f32 = (pixel_pos.x / camera.screen_size.x) * 2.0 - 1.0;
    let clip_y: f32 = -((pixel_pos.y / camera.screen_size.y) * 2.0 - 1.0);

    var out: VertexOutput;
    // frag_depth overrides this.
    out.clip_position = vec4f(clip_x, clip_y, 0.5, 1.0);
    out.atlas_uv = instance.uv_origin + quad_uv[idx] * instance.uv_size;
    out.tint = instance.tint;
    out.alpha = instance.alpha;
    out.remap_row = instance.remap_row;
    out.fx_flags = instance.fx_flags;
    out.fx_params = instance.fx_params;
    out.sinking_row = instance.sinking_row;
    out.world_pos = instance.position + local * instance.size;
    out.rect_top_height = select(
        vec2f(instance.position.y, instance.size.y),
        instance.z_rect,
        instance.z_rect.y > 0.0,
    );
    out.z_adjust = instance.z_adjust;
    out.z_gradient = instance.z_gradient;
    out.palette_light = instance.palette_light;
    return out;
}

// Native Z of row `row` (0 = top) of a blit; mirrors `native_z::sprite_row_z`
// and the copy in zsprite_shader.wgsl. The VXL cache blit walks the same
// gradient table (`VXL_CacheBlit @ 0x00707480` -> extended blitter).


fn native_candidate(in: VertexOutput) -> i32 {
    let camera_row: i32 = i32(round(camera.camera_pos.y + camera.native_z_origin_y));
    var rect_top: f32 = in.rect_top_height.x;
    var height: i32 = max(i32(round(in.rect_top_height.y)), 1);
    var gradient: u32 = in.z_gradient & 0xFFu;
    var z_adjust: i32 = i32(round(in.z_adjust));
    // Unit final composite 0x73B140: full-width upper h-16 then bottom 16,
    // seeded independently by Standard_SHP_blitter. A shared rectangle across
    // hull/turret/barrel makes their depth decision identical at every pixel.
    // Retain full quads/UVs so the split introduces no extra zoom-padding edge.
    if ((in.z_gradient & 0x400u) != 0u) {
        rect_top = round(rect_top);
        if (height > 16) {
            let boundary: f32 = rect_top + f32(height - 16);
            if (in.world_pos.y < boundary) {
                height = height - 16;
                gradient = 0u;
                z_adjust = z_adjust - 5;
            } else {
                rect_top = boundary;
                height = 16;
                gradient = 2u;
            }
        }
        // fx_params.w is this voxel body's tactical clip height in world
        // pixels, supplied from the actual scissor. Zero retains the whole
        // viewport for offscreen callers. Do not bake the sidebar/footer size
        // into this shader or infer it from atlas storage padding.
        let clip_height: f32 = select(
            camera.screen_size.y / camera.zoom,
            in.fx_params.w,
            in.fx_params.w > 0.0,
        );
        let region_top: f32 = rect_top;
        let region_bottom: f32 = rect_top + f32(height);
        let bottom: f32 = min(region_bottom, f32(camera_row) + round(clip_height));
        rect_top = max(rect_top, f32(camera_row));
        height = i32(bottom - rect_top);
        // VERA zoom padding belongs to the full quad. Re-testing untouched
        // natural edges against a smooth interpolant can discard a padded
        // source row through rounding (observed at 2x). Only actual tactical
        // clip intersections cut coverage; the existing row clamp owns natural
        // edges. Native 1x geometry and split/depth selection are unchanged.
        if (height <= 0
            || (rect_top > region_top && in.world_pos.y < rect_top)
            || (bottom < region_bottom && in.world_pos.y >= bottom)) {
            discard;
        }
    }
    let screen_top: i32 = i32(round(rect_top)) - camera_row;
    let row: i32 = clamp(i32(floor(in.world_pos.y - rect_top)), 0, height - 1);
    let z: i32 = native_row_z(gradient, screen_top, height, z_adjust, row);
    return z;
}

fn apply_fx(color: vec4f, _flags: u32, params: vec4f) -> vec4f {
    // Original location: `RA2-GAME.EXE-IDB` canon,
    // `rendering.drawStateEffects.ra2yr.json`; the same branch exists in the
    // SHP shader so representation does not affect active visual state.
    // Match the SHP path exactly: selector opacity, with no inferred EMP or
    // mirror styling. An effect on the object's light, such as the Iron
    // Curtain's tint, scales the intensity its PaletteLight carries
    // (natively GetEffectTintIntensity 0x0070E360 scales the intensity
    // argument, which selects the LightConvert row); only opacity is applied
    // here.
    return vec4f(color.rgb, color.a * params.x);
}


// Palette and stored-depth mechanisms are supplied by tactical_shader::source.


// RA2_DEBUG_DEPTH_VIEW (camera.pad1 > 0.5): depth as grey, wrapping every
// 128 world rows, so depth ordering can be read off a screenshot.
fn debug_depth_color(depth: f32) -> vec4f {
    let rows: f32 = world_row_from_native_depth(depth, camera.camera_pos.y + camera.native_z_origin_y);
    let g: f32 = fract(rows / 128.0);
    return vec4f(g, g, g, 1.0);
}

struct FragOutput {
    @location(0) color: vec4f,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_main(in: VertexOutput) -> FragOutput {
    // Unit73BEA4..73BF7B intersects the caller clip with the retained +3CA
    // waterline. Keep full geometry/UV/depth inputs: only fragment admission
    // changes. Discard suppresses both color and depth output (WGSL9.4.11:
    // https://www.w3.org/TR/WGSL/#discard-statement).
    if ((in.fx_flags & 128u) != 0u && in.world_pos.y >= in.sinking_row) {
        discard;
    }
    let atlas_size: vec2f = vec2f(textureDimensions(atlas));
    let atlas_coord: vec2i = vec2i(in.atlas_uv * atlas_size);
    let byte: u32 = textureLoad(atlas, atlas_coord, 0).r;

    // Color 0 = transparent (matches gamemd visibility-map invariant).
    if (byte == 0u) {
        discard;
    }

    var out: FragOutput;
    out.depth = stored_native_depth(native_candidate(in));

    // Ground shadow stencil (FX_SHADOW = 1 << 6): every non-zero atlas byte
    // darkens the destination. The native darken blitter halves the encoded
    // 16-bit word; this pass alpha-blends black in linear space against an
    // sRGB target, so the alpha that halves an encoded value is
    // 1 - 0.5^2.2 = 0.782 rather than 0.5 (the bridge shadow's 128/255 is a
    // recorded lighter drift; this path takes the closer value).
    if ((in.fx_flags & 64u) != 0u) {
        out.color = vec4f(0.0, 0.0, 0.0, 0.782 * in.alpha);
        return out;
    }

    // RGB substitution: bytes in [16, 32) sample the per-house ramp; all
    // others sample the theater palette directly.
    var rgb: vec3f;
    if (byte >= 16u && byte < 32u) {
        let ramp_coord: vec2i = vec2i(i32(byte - 16u), i32(in.remap_row));
        rgb = textureLoad(house_ramp, ramp_coord, 0).rgb;
    } else {
        let palette_coord: vec2i = vec2i(i32(byte), 0);
        rgb = textureLoad(palette, palette_coord, 0).rgb;
    }

    // The waterline changes geometry admission only. It must not opt an
    // otherwise opaque body out of the native packed-palette conversion.
    let color_flags = in.fx_flags & ~128u;
    var color: vec4f = vec4f(resolve_palette(rgb, in.tint, opaque_palette(in.palette_light, in.alpha, color_flags), byte, tactical_a_at(in.clip_position.xy)), in.alpha);
    color = apply_fx(color, in.fx_flags, in.fx_params);
    out.color = color;
    if (camera.pad1 > 0.5) {
        out.color = debug_depth_color(out.depth);
    }
    return out;
}
