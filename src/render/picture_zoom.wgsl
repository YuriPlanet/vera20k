// VERA-only, experimental (docs/picture-model-art.md): picture art drawn
// again at its own resolution over the magnified world (`picture_zoom.rs`).
//
// Each instance is a copy of a world instance that drew from a picture page;
// its UV addresses the page's HD twin, laid out at the same normalised
// coordinates. The quad is the world draw's quad, rounded as the world pass
// rounds it at zoom 1, then scaled into the target. A fragment maps back to
// the world pixel it covers, computes that pixel's native Z with the
// zsprite shader's row walker (`native_row_z`) and compares it with the
// world's stored depth there, so it passes exactly where the 1x draw passed.
// Palette light, the A plane and unlit indices go through the shared
// `palette_light.wgsl` functions (`tactical_shader::world_source`).

// The world camera (`BatchRenderer::set_native_world_resolution`: zoom 1,
// one target pixel per world pixel). Same layout as zsprite_shader.wgsl.
struct Camera {
    screen_size: vec2f,
    camera_pos: vec2f,
    zoom: f32,
    world_origin_y: f32,
    world_height: f32,
    pad1: f32,
    native_z_origin_y: f32,
    native_z_pad: f32,
};
@group(0) @binding(0) var<uniform> camera: Camera;

// The HD page: premultiplied encoded colour with mipmaps, and its index plane
// (0 hole, 1 lit, 240 unlit) at level 0.
@group(1) @binding(0) var t_hd: texture_2d<f32>;
@group(1) @binding(1) var s_hd: sampler;
@group(1) @binding(2) var hd_indices: texture_2d<u32>;

// Where the world lands in the target, and how the instance's run tests Z.
struct Params {
    // Target pixel of world-target pixel (0, 0), and target pixels per world
    // pixel.
    origin: vec2f,
    scale: f32,
    // 0: no Z test; 1: native Less (read-only leaves); 2: Less or equal
    // (read-write leaves, which stored their own Z).
    policy: u32,
    target_size: vec2f,
    // With `masked` 1, a target pixel whose frame pixel changed since the world
    // went in (UI drawn over it) keeps what it has (`WindowMask::Changed`).
    frame_scale: f32,
    masked: u32,
    frame_origin: vec2f,
    pad: vec2f,
};
@group(2) @binding(0) var world_depth: texture_depth_2d;
@group(2) @binding(1) var<uniform> params: Params;
@group(2) @binding(2) var frame_now: texture_2d<f32>;
@group(2) @binding(3) var frame_before: texture_2d<f32>;

// BUILDNGZ's remapped R8 canvas, as zsprite_shader.wgsl binds it.
@group(3) @binding(0) var t_zshape: texture_2d<f32>;

struct Instance {
    @location(0) position: vec2f,
    @location(1) size: vec2f,
    @location(2) uv_origin: vec2f,
    @location(3) uv_size: vec2f,
    @location(5) tint: vec3f,
    @location(6) alpha: f32,
    @location(8) fx_flags: u32,
    @location(11) z_adjust: f32,
    @location(12) z_gradient: u32,
    @location(13) zshape_origin: vec2f,
    @location(14) palette_light: vec4u,
};

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
    @location(1) @interpolate(flat) uv_rect: vec4f,
    @location(2) tint: vec3f,
    @location(3) @interpolate(flat) alpha: f32,
    @location(4) @interpolate(flat) fx_flags: u32,
    @location(5) @interpolate(flat) rect_top_height: vec2f,
    @location(6) @interpolate(flat) z_adjust: f32,
    @location(7) @interpolate(flat) z_gradient: u32,
    @location(8) @interpolate(flat) zshape_origin: vec2f,
    @location(9) @interpolate(flat) palette_light: vec4u,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32, instance: Instance) -> VertexOutput {
    var quad = array<vec2f, 6>(
        vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0),
    );
    let corner = quad[idx];
    // The world pass at zoom 1 rounds each corner to a whole target pixel
    // (zsprite_shader.wgsl `shp_vertex`); the magnified quad covers exactly
    // the window pixels of those world pixels.
    let world_target = floor(instance.position + corner * instance.size - camera.camera_pos + vec2f(0.5));
    let pixel = params.origin + world_target * params.scale;
    var output: VertexOutput;
    output.position = vec4f(
        pixel.x / params.target_size.x * 2.0 - 1.0,
        1.0 - pixel.y / params.target_size.y * 2.0,
        0.5,
        1.0,
    );
    output.uv = instance.uv_origin + corner * instance.uv_size;
    let far = instance.uv_origin + instance.uv_size;
    output.uv_rect = vec4f(min(instance.uv_origin, far), max(instance.uv_origin, far));
    output.tint = instance.tint;
    output.alpha = instance.alpha;
    output.fx_flags = instance.fx_flags;
    output.rect_top_height = vec2f(instance.position.y, instance.size.y);
    output.z_adjust = instance.z_adjust;
    output.z_gradient = instance.z_gradient;
    output.zshape_origin = instance.zshape_origin;
    output.palette_light = instance.palette_light;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4f {
    // Sampled first, in uniform control flow, for the mip level. The UV
    // stays half a texel inside the sprite so the filter does not reach a
    // neighbour on the page.
    let half_texel = 0.5 / vec2f(textureDimensions(t_hd));
    let uv = clamp(input.uv, input.uv_rect.xy + half_texel, input.uv_rect.zw - half_texel);
    let premultiplied = textureSample(t_hd, s_hd, uv);

    // The window pixel's world-target pixel: floor((window - origin) / scale).
    let world_target = (input.position.xy - params.origin) / params.scale;
    let texel = vec2i(floor(world_target));
    if params.masked != 0u {
        let last = vec2i(textureDimensions(frame_now)) - 1;
        let frame = clamp(vec2i(floor((input.position.xy - params.frame_origin) / params.frame_scale)), vec2i(0), last);
        if any(textureLoad(frame_now, frame, 0) != textureLoad(frame_before, frame, 0)) {
            discard;
        }
    }
    // A picture pixel is drawn when at least half covered (docs).
    if premultiplied.a < 0.5 {
        discard;
    }

    if params.policy != 0u {
        // The zsprite shader's `native_candidate` for this world pixel: the
        // blit rect's row walker, with the BUILDNGZ shape when it has one.
        let world_pos = vec2f(texel) + camera.camera_pos;
        let camera_row = i32(round(camera.camera_pos.y + camera.native_z_origin_y));
        var rect_top = i32(round(input.rect_top_height.x));
        var rect_bottom = rect_top + max(i32(round(input.rect_top_height.y)), 1);
        let extended_shape = (input.z_gradient & 0x100u) != 0u;
        var shape_delta = 0;
        if (input.z_gradient & 0x300u) != 0u {
            let origin = vec2i(input.zshape_origin);
            let canvas = vec2i(floor(world_pos)) - origin;
            let dims = vec2i(textureDimensions(t_zshape));
            if canvas.x < 0 || canvas.y < 0 || canvas.x >= dims.x || canvas.y >= dims.y {
                discard;
            }
            rect_top = max(rect_top, origin.y);
            rect_bottom = min(rect_bottom, origin.y + dims.y);
            if extended_shape {
                shape_delta = i32(textureLoad(t_zshape, canvas, 0).r * 255.0 + 0.5) - 128;
            }
        }
        let height = max(rect_bottom - rect_top, 1);
        let screen_top = rect_top - camera_row;
        let row = clamp(i32(floor(world_pos.y)) - rect_top, 0, height - 1);
        let entry = input.z_gradient & 0xFFu;
        let z_adjust = i32(round(input.z_adjust));
        var z = native_row_z(entry, screen_top, height, z_adjust, row);
        if extended_shape {
            if entry == 2u {
                z = ((32768 - height - screen_top + 1) & 0xFFFF) + z_adjust;
            } else {
                z = native_row_z(entry, screen_top, height, z_adjust, 0);
            }
            z = z - shape_delta;
        }
        let depth_last = vec2i(textureDimensions(world_depth)) - 1;
        let stored = textureLoad(world_depth, clamp(texel, vec2i(0), depth_last), 0);
        let candidate = stored_native_depth(z);
        // The world pass compares Less; a read-write leaf has since stored
        // its own Z there, which the HD copy must still pass.
        if candidate > stored || (params.policy == 1u && candidate == stored) {
            discard;
        }
    }

    let index_dims = vec2f(textureDimensions(hd_indices));
    let index_texel = vec2i(clamp(uv * index_dims, vec2f(0.0), index_dims - 1.0));
    // A filtered edge texel whose nearest texel is a hole is lit.
    let index = max(textureLoad(hd_indices, index_texel, 0).r, 1u);
    let encoded = premultiplied.rgb / premultiplied.a;
    let light = opaque_palette(input.palette_light, input.alpha, input.fx_flags);
    let rgb = resolve_palette(encoded, input.tint, light, index, tactical_a_at(world_target));
    return vec4f(rgb, 1.0);
}
