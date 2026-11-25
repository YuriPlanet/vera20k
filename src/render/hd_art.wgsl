// VERA's HD art, VERA-only: no gamemd counterpart. Draws the 2x
// upscaled art of buildings over the wheel-zoomed frame, where the native
// world still shows that building's own pixel.
//
// The world is composed at native pixels first, exactly as before; this pass
// only replaces pixels of the zoomed copy. For each frame pixel it finds the
// native texel under it and the building frame's palette index there:
//   * index 0 (transparent natively): untouched, so the native silhouette,
//     the ground and anything behind stay as they are;
//   * the native composed colour no longer matches the frame's own palette
//     colour under a plain light (a unit, an effect or a fog edge drew over
//     it): untouched;
//   * otherwise the HD pixel, brought to this palette row (theater palette,
//     house colour) and multiplied by the light the native pixel received.
//
// `srgb_encode` / `srgb_decode` are prepended by hd_art.rs.

struct Params {
    // Frame width, height, frame pixels per native pixel, debug view.
    frame: vec4f,
};

@group(0) @binding(0) var composition: texture_2d<f32>;
@group(0) @binding(1) var native_indices: texture_2d<u32>;
@group(0) @binding(2) var hd_art: texture_2d<f32>;
@group(0) @binding(3) var hd_sampler: sampler;
@group(0) @binding(4) var palette_rows: texture_2d<f32>;
@group(0) @binding(5) var reference_palette: texture_2d<f32>;
@group(0) @binding(6) var<uniform> params: Params;

struct Instance {
    // Native composition pixel of the frame's top-left corner.
    @location(0) origin: vec2f,
    // Native frame size in pixels.
    @location(1) size: vec2f,
    // The frame's top-left in the native index page (the HD page is 2x).
    @location(2) page_origin: vec2u,
    // Palette row the frame's indices resolve through (source_palette - 1).
    @location(3) row: u32,
};

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) origin: vec2f,
    @location(1) @interpolate(flat) size: vec2f,
    @location(2) @interpolate(flat) page_origin: vec2u,
    @location(3) @interpolate(flat) row: u32,
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
    out.page_origin = instance.page_origin;
    out.row = instance.row;
    return out;
}

fn luma(rgb: vec3f) -> f32 {
    return dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4f {
    let scale = params.frame.z;
    let native = floor(input.position.xy / scale);
    let local = native - input.origin;
    if any(local < vec2f(0.0)) || any(local >= input.size) {
        discard;
    }
    let texel = input.page_origin + vec2u(local);
    let index = textureLoad(native_indices, texel, 0).r;
    if index == 0u {
        discard;
    }
    // The frame's own colour in this palette row and in the reference
    // palette the art was upscaled with; both encoded.
    let own = textureLoad(palette_rows, vec2u(index, input.row), 0).rgb;
    let reference = textureLoad(reference_palette, vec2u(index, 0u), 0).rgb;

    // Is the native pixel still this frame's, under some plain light?
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

    // The HD pixel: 2 art pixels per native pixel, sampled at this frame
    // pixel's place.
    let art_size = vec2f(textureDimensions(hd_art));
    let art_px = vec2f(input.page_origin) * 2.0 + (input.position.xy / scale - input.origin) * 2.0;
    let art = textureSampleLevel(hd_art, hd_sampler, art_px / art_size, 0.0);
    if art.a < 0.5 {
        discard;
    }
    var encoded = srgb_encode(art.rgb);
    if index >= 16u && index < 32u {
        // House colour: the row's remap entry at the art's brightness. The
        // reference ramp is one hue (UNITTEM's is pure red), so its brightest
        // channel measures it down to the darkest step; luma would not.
        let peak = max(encoded.r, max(encoded.g, encoded.b));
        let reference_peak = max(reference.r, max(reference.g, reference.b));
        encoded = own * clamp(peak / max(reference_peak, 0.02), 0.0, 2.5);
    } else {
        // Theater palette: the row's difference from the reference.
        encoded = encoded + (own - reference);
    }
    let light = (composed + 0.01) / (base + 0.01);
    var rgb = srgb_decode(clamp(encoded, vec3f(0.0), vec3f(1.0))) * clamp(light, vec3f(0.0), vec3f(4.0));
    if params.frame.w == 1.0 {
        rgb = mix(rgb, vec3f(0.0, 1.0, 0.0), 0.35);
    }
    return vec4f(rgb, 1.0);
}
