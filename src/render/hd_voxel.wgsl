// VERA's HD art voxels, VERA-only: no gamemd counterpart. Draws the unit
// atlas's twice-resolution pictures (`vxl_raster::VxlHd`) over the
// wheel-zoomed frame, where the native world still shows that picture's own
// pixel.
//
// As `hd_art.wgsl`: the world is composed at native pixels first, exactly as
// before; this pass only replaces pixels of the zoomed copy. For each frame
// pixel it finds the native texel under it and the picture's palette index
// there:
//   * index 0 (transparent natively): untouched;
//   * the native composed colour no longer matches the picture's own colour
//     under a plain light (a building, an effect or a fog edge drew over
//     it): untouched;
//   * otherwise the HD picture's index there, in the same palette and house
//     ramp, multiplied by the light the native pixel received. An HD index
//     of 0 keeps the native pixel, so the silhouette never grows.
//
// `srgb_encode` / `srgb_decode` are prepended by hd_voxel.rs.

struct Params {
    // Frame width, height, frame pixels per native pixel, debug view.
    frame: vec4f,
};

@group(0) @binding(0) var composition: texture_2d<f32>;
@group(0) @binding(1) var native_page: texture_2d<u32>;
@group(0) @binding(2) var hd_page: texture_2d<u32>;
@group(0) @binding(3) var palette: texture_2d<f32>;
@group(0) @binding(4) var house_ramp: texture_2d<f32>;
@group(0) @binding(5) var<uniform> params: Params;

struct Instance {
    // Native composition pixel of the picture's top-left corner.
    @location(0) origin: vec2f,
    // Native picture size in pixels.
    @location(1) size: vec2f,
    // The picture's rectangle on its atlas page, 0..1.
    @location(2) uv_origin: vec2f,
    @location(3) uv_size: vec2f,
    // DrawState::remap_row and the source palette row, as the voxel shader.
    @location(4) remap_row: u32,
    @location(5) source_palette: u32,
};

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) origin: vec2f,
    @location(1) @interpolate(flat) size: vec2f,
    @location(2) @interpolate(flat) uv_origin: vec2f,
    @location(3) @interpolate(flat) uv_size: vec2f,
    @location(4) @interpolate(flat) remap_row: u32,
    @location(5) @interpolate(flat) source_palette: u32,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32, instance: Instance) -> VertexOutput {
    let corner = vec2f(f32(index & 1u), f32((index >> 1u) & 1u));
    let frame_px = (instance.origin + corner * instance.size) * params.frame.z;
    var out: VertexOutput;
    out.position = vec4f(
        frame_px.x / params.frame.x * 2.0 - 1.0,
        1.0 - frame_px.y / params.frame.y * 2.0,
        0.0,
        1.0,
    );
    out.origin = instance.origin;
    out.size = instance.size;
    out.uv_origin = instance.uv_origin;
    out.uv_size = instance.uv_size;
    out.remap_row = instance.remap_row;
    out.source_palette = instance.source_palette;
    return out;
}

fn luma(rgb: vec3f) -> f32 {
    return dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
}

// The encoded colour of palette `index`, as `voxel_source_rgb` in
// sprite_voxel_shader.wgsl.
fn source_rgb(index: u32, remap_row: u32, source_palette: u32) -> vec3f {
    if source_palette == 0u && index >= 16u && index < 32u {
        return textureLoad(house_ramp, vec2i(i32(index - 16u), i32(remap_row)), 0).rgb;
    }
    return textureLoad(palette, vec2i(i32(index), i32(source_palette)), 0).rgb;
}

// The texel of `page` under picture point `local` (native pixels from the
// picture's corner, fractional).
fn page_texel(page_size: vec2f, input: VertexOutput, local: vec2f) -> vec2u {
    let uv = input.uv_origin + local / input.size * input.uv_size;
    return vec2u(clamp(floor(uv * page_size), vec2f(0.0), page_size - 1.0));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4f {
    let scale = params.frame.z;
    let native = floor(input.position.xy / scale);
    let local = native - input.origin;
    if any(local < vec2f(0.0)) || any(local >= input.size) {
        discard;
    }
    let native_size = vec2f(textureDimensions(native_page));
    let index = textureLoad(native_page, page_texel(native_size, input, local + 0.5), 0).r;
    if index == 0u {
        discard;
    }
    let own = source_rgb(index, input.remap_row, input.source_palette);

    // Is the native pixel still this picture's, under some plain light?
    let composed = textureLoad(composition, vec2u(native), 0).rgb;
    let base = srgb_decode(own);
    let base_l = luma(base);
    let composed_l = luma(composed);
    if base_l < 0.004 {
        if composed_l > 0.02 {
            discard;
        }
    } else {
        let k = composed_l / base_l;
        let error = length(composed - base * k) / max(composed_l, 0.02);
        if k < 0.15 || k > 4.0 || error > 0.2 {
            discard;
        }
    }

    // The HD picture: two texels per native pixel.
    let hd_size = vec2f(textureDimensions(hd_page));
    let fine = input.position.xy / scale - input.origin;
    let hd_index = textureLoad(hd_page, page_texel(hd_size, input, fine), 0).r;
    if hd_index == 0u {
        discard;
    }
    let encoded = source_rgb(hd_index, input.remap_row, input.source_palette);
    let light = (composed + 0.01) / (base + 0.01);
    var rgb = srgb_decode(encoded) * clamp(light, vec3f(0.0), vec3f(4.0));
    if params.frame.w == 1.0 {
        rgb = mix(rgb, vec3f(0.0, 1.0, 1.0), 0.35);
    }
    return vec4f(rgb, 1.0);
}
