// Camera uniform: viewport size, scroll position, and depth parameters.
struct Camera {
    screen_size: vec2f,
    camera_pos: vec2f,
    // Zoom level: 1.0 = native, >1.0 = zoomed in, <1.0 = zoomed out.
    zoom: f32,
    world_origin_y: f32,
    world_height: f32,
    pad0: f32,
    native_z_origin_y: f32,
    native_z_pad: f32,
};
@group(0) @binding(0) var<uniform> camera: Camera;

// The source page bindings (nearest-neighbor sampler for pixel art) are
// supplied by sprite_source.wgsl.

// Per-instance data from the instance buffer.
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
    @location(14) palette_light: vec4u,
    @location(15) source_palette: u32,
};

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
    @location(1) tint: vec3f,
    @location(2) alpha: f32,
    @location(3) @interpolate(flat) fx_flags: u32,
    @location(4) fx_params: vec4f,
    @location(11) @interpolate(flat) palette_light: vec4u,
    @location(13) @interpolate(flat) source_palette: u32,
};

fn vertex_impl(
    idx: u32,
    instance: Instance,
) -> VertexOutput {
    // Quad vertex positions: (0,0) top-left to (1,1) bottom-right.
    var quad_pos = array<vec2f, 6>(
        vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0),
    );
    var quad_uv = array<vec2f, 6>(
        vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0),
    );

    let local: vec2f = quad_pos[idx];
    // Keep the source-to-screen scale exact. Padding a quad while keeping its
    // UV range stretches every sprite independently, distorting pixel spacing
    // and misaligning neighboring terrain texels. Nearest sampling needs no
    // extra coverage; shared world boundaries project to the same screen edge.
    let is_zoomed: bool = abs(camera.zoom - 1.0) >= 0.001;
    let raw_pos: vec2f = (instance.position + local * instance.size - camera.camera_pos) * camera.zoom;
    let pixel_pos: vec2f = select(raw_pos, floor(raw_pos + vec2f(0.5, 0.5)), !is_zoomed);

    // Convert pixel coordinates to clip space.
    // Screen: (0,0) = top-left, (screen_size) = bottom-right.
    // Clip: (-1,-1) = bottom-left, (1,1) = top-right.
    let clip_x: f32 = (pixel_pos.x / camera.screen_size.x) * 2.0 - 1.0;
    let clip_y: f32 = 1.0 - (pixel_pos.y / camera.screen_size.y) * 2.0;

    var output: VertexOutput;
    output.position = vec4f(clip_x, clip_y, instance.depth, 1.0);
    output.uv = instance.uv_origin + quad_uv[idx] * instance.uv_size;
    output.tint = instance.tint;
    output.alpha = instance.alpha;
    output.fx_flags = instance.fx_flags;
    output.fx_params = instance.fx_params;
    output.palette_light = instance.palette_light;
    output.source_palette = instance.source_palette;
    return output;
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32, instance: Instance) -> VertexOutput {
    return vertex_impl(idx, instance);
}

// Shared native storage conversion is used only by pipelines that test Z.
@vertex
fn vs_depth(@builtin(vertex_index) idx: u32, instance: Instance) -> VertexOutput {
    var output = vertex_impl(idx, instance);
    output.position.z = compatibility_depth_on_native_axis(
        instance.depth, camera.camera_pos.y + camera.native_z_origin_y, camera.world_origin_y, camera.world_height);
    return output;
}

fn apply_fx(color: vec4f, _flags: u32, params: vec4f) -> vec4f {
    // Original location: `RA2-GAME.EXE-IDB` canon,
    // `rendering.drawStateEffects.ra2yr.json`; this representation-neutral
    // branch mirrors the voxel shader so SHP and VXL share one DrawState ABI.
    // YR resolves selector opacity before either SHP or VXL submission. EMP
    // and mirror deliberately remain no-op residuals. An effect on the
    // object's light, such as the Iron Curtain's tint, scales the intensity
    // its PaletteLight carries (natively GetEffectTintIntensity 0x0070E360
    // scales the intensity argument, which selects the LightConvert row);
    // only opacity is applied here.
    return vec4f(color.rgb, color.a * params.x);
}


// Color resolution is supplied by palette_light::shader_source.

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4f {
    let index: u32 = textureLoad(source_indices, source_texel(input.uv), 0).r;
    var color: vec4f = textureSample(t_sprite, s_sprite, input.uv);
    if (input.source_palette != 0u) {
        color = indexed_color(index, input.source_palette);
    }
    // Discard fully transparent pixels so they don't write to the depth buffer.
    // Without this, transparent regions of sprite quads would occlude objects behind them.
    if (color.a < 0.01) {
        discard;
    }
    // Map lighting happens before the shared DrawState effect branch, matching
    // the voxel fragment path. Alpha 1.0 = opaque; no draw state changes order.
    return apply_fx(
        vec4f(resolve_palette(color.rgb, input.tint, opaque_palette(input.palette_light, color.a * input.alpha, input.fx_flags), index, tactical_a_at(input.position.xy)), color.a * input.alpha),
        input.fx_flags,
        input.fx_params,
    );
}
