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
    // The quad's UV rectangle, which only `fs_shell_smooth` reads.
    @location(5) @interpolate(flat) source_rect: vec4f,
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
    output.source_rect = vec4f(instance.uv_origin, instance.uv_size);
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
    return shade(input, tactical_a_at(input.position.xy));
}

// DSurface4C0750's dashed stores (rally passes, psychic detection dashes)
// keep or skip each pixel by the live tactical A: FX_SURFACE_A_NONZERO
// (256) stores where it is nonzero, FX_SURFACE_A_ZERO (512) where it is zero,
// and neither stores everywhere. The stored color reads no palette row.
@fragment
fn fs_surface_store(input: VertexOutput) -> @location(0) vec4f {
    var store = input;
    store.fx_flags = input.fx_flags & ~768u;
    let color = shade(store, 127u);
    let a = tactical_a_at(input.position.xy);
    if ((input.fx_flags & 256u) != 0u && a == 0u) || ((input.fx_flags & 512u) != 0u && a != 0u) {
        discard;
    }
    return color;
}

fn shade(input: VertexOutput, a: u32) -> vec4f {
    let index: u32 = textureLoad(source_indices, source_texel(input.uv), 0).r;
    return shade_color(input, sampled_source_color(input.uv, index, input.source_palette), index, a);
}

// What an encoded source colour stores: the shared end of every colour path here.
fn shade_color(input: VertexOutput, color: vec4f, index: u32, a: u32) -> vec4f {
    // Discard fully transparent pixels so they don't write to the depth buffer.
    // Without this, transparent regions of sprite quads would occlude objects behind them.
    if (color.a < 0.01) {
        discard;
    }
    // Map lighting happens before the shared DrawState effect branch, matching
    // the voxel fragment path. Alpha 1.0 = opaque; no draw state changes order.
    return apply_fx(
        vec4f(resolve_palette(color.rgb, input.tint, opaque_palette(input.palette_light, color.a * input.alpha, input.fx_flags), index, a), color.a * input.alpha),
        input.fx_flags,
        input.fx_params,
    );
}

// VERA's smooth online client (`[VERA] SmoothOnlineMenus=`, off by default),
// for RGBA chrome pages. Nearest sampling at a fractional scale gives source
// pixels uneven widths, and stair-steps on every curve. Here each enlarged
// pixel is the box-filtered average of the source pixels under it, so its
// pixels stay even at any scale, plus 40% of the rounding xBR gives corners.
// A quad drawn at its own size stays exactly as it was. Text and the cursor
// never come here.
//
// `smooth_corner` is one turn of the corner test in `render::xbr`: FFmpeg's
// vf_xbr rule, with its colour distance and threshold. FFmpeg's blends fill
// fixed sub-pixel slots for 2x, 3x and 4x; this draws the same corners as
// lines through the source pixel, as Hyllian's xBR-lv2 shader does, so they
// fit any scale. Presentation only.

// Source pixels a screen pixel spans, at and above which a quad is not enlarged.
const SMOOTH_NATIVE_FOOTPRINT: f32 = 0.995;
// How far xBR moves a pixel, as `render::sidebar_smoothing`'s STRENGTH.
const SMOOTH_STRENGTH: f32 = 0.4;
// FFmpeg's `eq` threshold of 155, on these 0..1 channels.
const SMOOTH_EQUAL: f32 = 0.6078;
// A corner takes the nearer of its two neighbours' colours, so where either
// neighbour is closer than this its rounding would move the pixel by at most
// 40% of this. Too little to see, and leaving those pixels out spares the
// costly part of the test on most of the art's fine grain.
const SMOOTH_CONTRAST: f32 = 0.06;

struct SmoothTexel {
    // Premultiplied so that clear pixels add no colour. Square roots stand in
    // for sRGB's curve: xBR and its mix work on stored bytes, not on light.
    color: vec4f,
    // FFmpeg's YUV, and alpha so a clear pixel differs from a black one.
    yuva: vec4f,
}

struct SmoothCorner {
    px: vec4f,
    blend: f32,
}

// The texel `offset` from `cell`, kept inside the quad's source rectangle:
// past it a piece's edge repeats, as `render::xbr` reads past a piece's edge.
fn smooth_texel(cell: vec2f, offset: vec2f, bounds: vec4f) -> SmoothTexel {
    let source = textureLoad(t_sprite, vec2i(clamp(cell + offset, bounds.xy, bounds.zw)), 0);
    let rgb = sqrt(source.rgb) * source.a;
    return SmoothTexel(
        vec4f(rgb, source.a),
        vec4f(
            dot(rgb, vec3f(0.299, 0.587, 0.114)),
            dot(rgb, vec3f(-0.169, -0.331, 0.5)),
            dot(rgb, vec3f(0.5, -0.419, -0.081)),
            source.a,
        ),
    );
}

fn smooth_df(a: SmoothTexel, b: SmoothTexel) -> f32 {
    return dot(abs(a.yuva - b.yuva), vec4f(1.0));
}

fn smooth_eq(a: SmoothTexel, b: SmoothTexel) -> bool {
    return smooth_df(a, b) < SMOOTH_EQUAL;
}

fn smooth_same(a: SmoothTexel, b: SmoothTexel) -> bool {
    return all(a.yuva == b.yuva);
}

// One turn of the rule, named as in `render::xbr::corner`, for a corner whose
// `ph` and `pf` both differ from `pe`. `right` and `down` are the turn's axes
// in the page. `at` is the fragment's place in the source pixel with the
// corner this turn rounds at (1, 1); `pixel` is a screen pixel's size there.
fn smooth_corner(
    cell: vec2f, bounds: vec4f, right: vec2f, down: vec2f,
    pe: SmoothTexel, pi: SmoothTexel, ph: SmoothTexel, pf: SmoothTexel,
    pg: SmoothTexel, pc: SmoothTexel, pd: SmoothTexel, pb: SmoothTexel,
    at: vec2f, pixel: f32,
) -> SmoothCorner {
    var corner: SmoothCorner;
    let f4 = smooth_texel(cell, 2.0 * right, bounds);
    let i4 = smooth_texel(cell, 2.0 * right + down, bounds);
    let h5 = smooth_texel(cell, 2.0 * down, bounds);
    let i5 = smooth_texel(cell, right + 2.0 * down, bounds);
    let e = smooth_df(pe, pc) + smooth_df(pe, pg) + smooth_df(pi, h5) + smooth_df(pi, f4)
        + 4.0 * smooth_df(ph, pf);
    let i = smooth_df(ph, pd) + smooth_df(ph, i5) + smooth_df(pf, i4) + smooth_df(pf, pb)
        + 4.0 * smooth_df(pe, pi);
    if e > i {
        return corner;
    }
    corner.px = select(ph.color, pf.color, smooth_df(pe, pf) <= smooth_df(pe, ph));
    // Each edge is a line through the pixel, anti-aliased over a screen pixel;
    // the unsure case rounds only the corner's tip.
    let sum = at.x + at.y;
    corner.blend = saturate((sum + pixel - 1.75) / (2.0 * pixel));
    let sharp = e < i
        && ((!smooth_eq(pf, pb) && !smooth_eq(ph, pd))
            || (smooth_eq(pe, pi) && !smooth_eq(pf, i4) && !smooth_eq(ph, i5))
            || smooth_eq(pe, pg)
            || smooth_eq(pe, pc));
    if sharp {
        corner.blend = max(corner.blend, saturate((sum + pixel - 1.5) / (2.0 * pixel)));
        let ke = smooth_df(pf, pg);
        let ki = smooth_df(ph, pc);
        if 2.0 * ke <= ki && !smooth_same(pe, pg) && !smooth_same(pd, pg) {
            let line = at.y + 0.5 * at.x;
            corner.blend = max(corner.blend, saturate((line + 0.5 * pixel - 1.0) / pixel));
        }
        if ke >= 2.0 * ki && !smooth_same(pe, pc) && !smooth_same(pb, pc) {
            let line = at.y + 2.0 * at.x;
            corner.blend = max(corner.blend, saturate((line + pixel - 2.0) / (2.0 * pixel)));
        }
    }
    return corner;
}

// A premultiplied smooth colour as the linear straight alpha `shade_color` takes.
fn smooth_store(color: vec4f) -> vec4f {
    let rgb = saturate(color.rgb / max(color.a, 0.001));
    return vec4f(rgb * rgb, saturate(color.a));
}

// The smoothed colour at `position` in source pixels. `rect` is the quad's
// source rectangle in pixels and `scale` the screen pixels per source pixel.
fn smooth_chrome(position: vec2f, rect: vec4f, scale: vec2f) -> vec4f {
    let cell = floor(position);
    let at = position - cell;
    let first = floor(rect.xy + vec2f(0.001));
    let bounds = vec4f(first, max(ceil(rect.xy + rect.zw - vec2f(0.001)) - vec2f(1.0), first));

    let pa = smooth_texel(cell, vec2f(-1.0, -1.0), bounds);
    let pb = smooth_texel(cell, vec2f(0.0, -1.0), bounds);
    let pc = smooth_texel(cell, vec2f(1.0, -1.0), bounds);
    let pd = smooth_texel(cell, vec2f(-1.0, 0.0), bounds);
    let pe = smooth_texel(cell, vec2f(0.0, 0.0), bounds);
    let pf = smooth_texel(cell, vec2f(1.0, 0.0), bounds);
    let pg = smooth_texel(cell, vec2f(-1.0, 1.0), bounds);
    let ph = smooth_texel(cell, vec2f(0.0, 1.0), bounds);
    let pi = smooth_texel(cell, vec2f(1.0, 1.0), bounds);

    // How much of the screen pixel lies over the source pixels before and
    // after this one along each axis.
    let before = saturate(vec2f(0.5) - at * scale);
    let after = saturate((at - vec2f(1.0)) * scale + vec2f(0.5));
    let middle = vec2f(1.0) - before - after;
    let box_filtered = (pa.color * before.x + pb.color * middle.x + pc.color * after.x) * before.y
        + (pd.color * before.x + pe.color * middle.x + pf.color * after.x) * middle.y
        + (pg.color * before.x + ph.color * middle.x + pi.color * after.x) * after.y;

    // A stretched bar's corners would smear along it, and a corner needs two
    // neighbours that meet there and differ from this pixel; most have none.
    let differs_h = smooth_df(pe, ph) > SMOOTH_CONTRAST;
    let differs_f = smooth_df(pe, pf) > SMOOTH_CONTRAST;
    let differs_b = smooth_df(pe, pb) > SMOOTH_CONTRAST;
    let differs_d = smooth_df(pe, pd) > SMOOTH_CONTRAST;
    let round_bottom_right = differs_h && differs_f;
    let round_top_right = differs_f && differs_b;
    let round_top_left = differs_b && differs_d;
    let round_bottom_left = differs_d && differs_h;
    if max(scale.x, scale.y) > 2.0 * min(scale.x, scale.y)
        || !(round_bottom_right || round_top_right || round_top_left || round_bottom_left)
    {
        return smooth_store(box_filtered);
    }

    let pixel = 2.0 / (scale.x + scale.y);
    var bottom_right: SmoothCorner;
    var top_right: SmoothCorner;
    var top_left: SmoothCorner;
    var bottom_left: SmoothCorner;
    if round_bottom_right {
        bottom_right = smooth_corner(cell, bounds, vec2f(1.0, 0.0), vec2f(0.0, 1.0),
            pe, pi, ph, pf, pg, pc, pd, pb, at, pixel);
    }
    if round_top_right {
        top_right = smooth_corner(cell, bounds, vec2f(0.0, -1.0), vec2f(1.0, 0.0),
            pe, pc, pf, pb, pi, pa, ph, pd, vec2f(1.0 - at.y, at.x), pixel);
    }
    if round_top_left {
        top_left = smooth_corner(cell, bounds, vec2f(-1.0, 0.0), vec2f(0.0, -1.0),
            pe, pa, pb, pd, pc, pg, pf, ph, vec2f(1.0) - at, pixel);
    }
    if round_bottom_left {
        bottom_left = smooth_corner(cell, bounds, vec2f(0.0, 1.0), vec2f(-1.0, 0.0),
            pe, pg, pd, ph, pa, pi, pb, pf, vec2f(at.y, 1.0 - at.x), pixel);
    }

    // Opposite corners combine; of the two diagonals, the one that moves this
    // pixel most is the edge it lies on.
    let falling = mix(mix(pe.color, bottom_right.px, bottom_right.blend), top_left.px, top_left.blend);
    let rising = mix(mix(pe.color, top_right.px, top_right.blend), bottom_left.px, bottom_left.blend);
    let rounded = select(
        rising,
        falling,
        dot(abs(falling - pe.color), vec4f(1.0)) > dot(abs(rising - pe.color), vec4f(1.0)),
    );
    return smooth_store(box_filtered + SMOOTH_STRENGTH * (rounded - pe.color));
}

@fragment
fn fs_shell_smooth(input: VertexOutput) -> @location(0) vec4f {
    let dims = vec2f(textureDimensions(t_sprite));
    let position = input.uv * dims;
    let footprint = abs(vec2f(dpdx(position.x), dpdy(position.y)));
    var color = textureLoad(t_sprite, vec2i(position), 0);
    if any(footprint < vec2f(SMOOTH_NATIVE_FOOTPRINT)) {
        color = smooth_chrome(
            position,
            input.source_rect * dims.xyxy,
            vec2f(1.0) / clamp(footprint, vec2f(0.001), vec2f(1.0)),
        );
    }
    let index: u32 = textureLoad(source_indices, source_texel(input.uv), 0).r;
    let encoded = vec4f(srgb_encode(color.rgb), color.a);
    return shade_color(input, encoded, index, tactical_a_at(input.position.xy));
}
