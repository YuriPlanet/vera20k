// Per-pixel Z-tested SHP sprite shader (gamemd `0x2E00` / `0x6E00` draws).
//
// Same vertex mapping as batch_shader.wgsl. The fragment stage recomputes the
// native 16-bit Z of its row from the blit rect's screen top, the gradient
// entry and the class Z term (`native_z.rs` is the Rust twin of this math),
// subtracts the optional z-shape byte when the instance carries one, and maps
// the integer Z onto the normalised depth axis: one native Z unit is one world
// pixel row. Two pipelines share it: depth compare Less without write (every
// object except buildings: leaves 0x00494b60 / 0x00497fd0, `fs_main`) and
// Less with write (building bodies: leaves 0x004958d0 / 0x004990e0,
// `fs_write`).

struct Camera {
    screen_size: vec2f,
    camera_pos: vec2f,
    zoom: f32,
    // Depth axis: depth = 1 - (row - world_origin_y) / world_height.
    world_origin_y: f32,
    world_height: f32,
    pad1: f32,
    native_z_origin_y: f32,
    native_z_pan: f32,
};
@group(0) @binding(0) var<uniform> camera: Camera;

// The source page bindings are supplied by sprite_source.wgsl.

// BUILDNGZ's remapped R8 canvas, or four raw overlay files in RGBA channels.
// Both store signed bytes biased by128. Overlay instances carry their stride.
@group(2) @binding(0) var t_zshape: texture_2d<f32>;
// The skip table (render::zshape_skips): per slot (row) and frame row
// (column), the columns a left-edge body's z-shape pointer trails by.
@group(2) @binding(1) var t_zshape_skips: texture_2d<u32>;

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
    @location(11) z_adjust: f32,
    @location(12) z_gradient: u32,
    @location(13) zshape_origin: vec2f,
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
    // World-pixel position of this fragment (unpadded quad).
    @location(6) world_pos: vec2f,
    // Blit rect left, top, width and height in world pixels.
    @location(7) @interpolate(flat) rect: vec4f,
    @location(8) @interpolate(flat) z_adjust: f32,
    @location(9) @interpolate(flat) z_gradient: u32,
    @location(10) @interpolate(flat) zshape_origin: vec2f,
    @location(11) @interpolate(flat) palette_light: vec4u,
    // Only vs_ordinal supplies an ordered destination command ordinal.
    @location(12) @interpolate(flat) command_ordinal: u32,
    @location(13) @interpolate(flat) source_palette: u32,
};

fn shp_vertex(idx: u32, instance: Instance) -> VertexOutput {
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
    // Match the exact source scale used by the other tactical blitters.
    let raw_pos: vec2f = (instance.position + local * instance.size - camera.camera_pos) * camera.zoom;
    let pixel_pos: vec2f = select(raw_pos, floor(raw_pos + vec2f(0.5, 0.5)), !is_zoomed);

    let clip_x: f32 = (pixel_pos.x / camera.screen_size.x) * 2.0 - 1.0;
    let clip_y: f32 = 1.0 - (pixel_pos.y / camera.screen_size.y) * 2.0;

    var output: VertexOutput;
    // frag_depth overrides this.
    output.position = vec4f(clip_x, clip_y, 0.5, 1.0);
    output.uv = instance.uv_origin + quad_uv[idx] * instance.uv_size;
    output.tint = instance.tint;
    output.alpha = instance.alpha;
    output.fx_flags = instance.fx_flags;
    output.fx_params = instance.fx_params;
    output.world_pos = instance.position + local * instance.size;
    output.rect = vec4f(instance.position, instance.size);
    output.z_adjust = instance.z_adjust;
    output.z_gradient = instance.z_gradient;
    output.zshape_origin = instance.zshape_origin;
    output.palette_light = instance.palette_light;
    output.source_palette = instance.source_palette;
    return output;
}

// Ordinary SHP consumers retain their original vertex index and projection.
@vertex
fn vs_main(@builtin(vertex_index) idx: u32, instance: Instance) -> VertexOutput {
    return shp_vertex(idx, instance);
}

// The first vertex carries command sequence, independently of instance index:
// draw(ordinal*6..ordinal*6+6, original_instance..original_instance+1).
@vertex
fn vs_ordinal(@builtin(vertex_index) idx: u32, instance: Instance) -> VertexOutput {
    var output = shp_vertex(idx % 6u, instance);
    output.command_ordinal = idx / 6u + 1u;
    return output;
}

// The view's world rows for a tactical view `view_height` world pixels high
// (fx_params.w).
fn tactical_view_rows(view_height: f32) -> vec2i {
    return view_rows(camera.camera_pos.y + camera.native_z_origin_y, view_height,
        camera.screen_size.y / camera.zoom);
}

// One signed candidate owner for ordinary and packed SHP consumers follows.
// terrain_draw.rs `edit_source` reuses the shader above this line.

// Source columns the RLE ZReadWrite leaf (0x004990E0) leaves its z-shape
// pointer behind by in this fragment's row of an extended z-shape body the
// view's left edge cuts (native_z::zshape_skip_overshoot, one byte per frame
// row in the skip table; fx_params.w is the body's slot, 1-based, 0 none).
// The columns left of the skip get it too; the view clips them anyway.
fn skip_overshoot(input: VertexOutput) -> i32 {
    let slot: i32 = i32(round(input.fx_params.w)) - 1;
    let row: i32 = i32(floor(input.world_pos.y)) - i32(round(input.rect.y));
    let dims: vec2i = vec2i(textureDimensions(t_zshape_skips));
    if (slot < 0 || slot >= dims.y || row < 0 || row >= dims.x) {
        return 0;
    }
    return i32(textureLoad(t_zshape_skips, vec2i(row, slot), 0).r);
}

fn native_candidate(input: VertexOutput) -> i32 {
    let z_origin: i32 = native_z_origin_row(camera.camera_pos.y + camera.native_z_origin_y, camera.native_z_pan);
    // A turreted SHP Unit's body and turret reach the screen through the
    // Unit composite (`0x0073B140`) as a voxel body does, on the composite's
    // rectangle (zshape_origin; native_z::Z_GRADIENT_VOXEL_VIEW_CLIP_FLAG).
    // An overlay's linear shape (0x800) uses bit 0x1000 for its channel.
    if ((input.z_gradient & 0x1800u) == 0x1000u) {
        let candidate: vec2i = unit_composite_candidate(input.zshape_origin, input.world_pos.y,
            input.z_gradient, input.z_adjust, tactical_view_rows(input.fx_params.w), z_origin);
        if (candidate.y == 0) {
            discard;
        }
        return candidate.x;
    }
    var rect_top: i32 = i32(round(input.rect.y));
    var rect_bottom: i32 = rect_top + max(i32(round(input.rect.w)), 1);
    let extended_shape: bool = (input.z_gradient & 0x100u) != 0u;
    // RESIDUAL: without a z-shape the RLE walker (raw frames' Bit_Blit has no
    // shape) reads its per-pixel adjust from the 0x100-byte zero row at
    // 0x0089C568 (0x00437CCC), from the first column it draws. Past 256 of
    // them it reads the globals that follow: the bombs vector at 0x0089C668,
    // whose heap pointer and counts change at run time, and beyond, each byte
    // a signed adjust of up to 127. VERA keeps 0 there, so an RLE frame drawn
    // with Z more than 256 columns wide sits up to 127 Z off from its 256th
    // drawn column on.
    var shape_delta: i32 = 0;
    if ((input.z_gradient & 0x800u) != 0u) {
        let stride = i32(input.z_gradient >> 16u);
        let delta = vec2i(floor(input.world_pos)) - vec2i(input.zshape_origin);
        let index = delta.y * stride + delta.x;
        let dims = vec2i(textureDimensions(t_zshape));
        // Native raw surface addressing can cross row boundaries and even
        // the file allocation. VERA uses signed zero beyond file bytes;
        // per-channel padding and this guard make that boundary explicit.
        if (index >= 0 && index < dims.x * dims.y) {
            let values = textureLoad(t_zshape, vec2i(index % dims.x, index / dims.x), 0);
            let channel = (input.z_gradient >> 12u) & 3u;
            shape_delta = i32(values[channel] * 255.0 + 0.5) - 128;
        }
    } else if ((input.z_gradient & 0x300u) != 0u) {
        let origin: vec2i = vec2i(input.zshape_origin);
        let canvas: vec2i = vec2i(floor(input.world_pos)) - origin;
        let dims: vec2i = vec2i(textureDimensions(t_zshape));
        // CC_Draw_Shape @ 0x4AF08C..0x4AF0FB intersects the body and second
        // shape before dispatching either walker. The clipped rect supplies
        // the seed too; merely discarding outside pixels keeps a wrong seed
        // when the shape cuts off the bottom of the body.
        if (canvas.x < 0 || canvas.y < 0 || canvas.x >= dims.x || canvas.y >= dims.y) {
            discard;
        }
        rect_top = max(rect_top, origin.y);
        rect_bottom = min(rect_bottom, origin.y + dims.y);
        if (extended_shape) {
            // The z-shape pointer runs on linearly, a left-edge skip's
            // overshoot behind the pixels it pairs with.
            let index: i32 = canvas.y * dims.x + canvas.x - skip_overshoot(input);
            if (index >= 0) {
                let texel: vec2i = vec2i(index % dims.x, index / dims.x);
                shape_delta = i32(textureLoad(t_zshape, texel, 0).r * 255.0 + 0.5) - 128;
            }
        }
    }
    // Bit_Blit seeds a raw frame from its rect clipped to the view, the RLE
    // walker a top-seeded entry from the clipped top (native_z.wgsl).
    let seed: vec2i = shp_seed_rows(vec2i(rect_top, rect_bottom), input.z_gradient,
        tactical_view_rows(input.fx_params.w));
    let height: i32 = max(seed.y - seed.x, 1);
    let screen_top: i32 = seed.x - z_origin;
    let row: i32 = clamp(i32(floor(input.world_pos.y)) - seed.x, 0, height - 1);
    let entry: u32 = input.z_gradient & 0xFFu;
    var z: i32 = native_row_z(entry, screen_top, height, i32(round(input.z_adjust)), row);
    if (extended_shape) {
        // 437BAB keeps top-gradient quantisation;437C39 bypasses it only
        // for the bottom gradient. Shape rows do not step the seeded Z.
        if (entry == 2u) {
            z = ((32768 - height - screen_top + 1) & 0xFFFF) + i32(round(input.z_adjust));
        } else {
            z = native_row_z(entry, screen_top, height, i32(round(input.z_adjust)), 0);
        }
        z = z - shape_delta;
    }

    return z;
}

fn apply_fx(color: vec4f, _flags: u32, params: vec4f) -> vec4f {
    return vec4f(color.rgb, color.a * params.x);
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

// What an admitted source pixel stores. Both discards stay at the depth they
// have here: one more call level around them makes optimizing FXC stop with an
// internal error (terrain_depth.wgsl).
fn zsprite_store(input: VertexOutput, color: vec4f, index: u32, z: i32) -> FragOutput {
    // The shared attachment stores the low16 native word, independent of map bounds.
    let frag_depth: f32 = stored_native_depth(z);

    var output: FragOutput;
    output.color = apply_fx(
        vec4f(resolve_palette(color.rgb, input.tint, opaque_palette(input.palette_light, color.a * input.alpha, input.fx_flags), index, tactical_a_at(input.position.xy)), color.a * input.alpha),
        input.fx_flags,
        input.fx_params,
    );
    output.depth = frag_depth;
    if (camera.pad1 > 0.5) {
        output.color = debug_depth_color(frag_depth);
    }
    return output;
}

// The Z leaves compare the signed candidate before storing its low 16 bits
// (draw iff cand < stored; RLE 0x00497FD0 and 0x004990E0, raw 0x00494B60 and
// 0x004958D0), while the depth test compares stored words. A candidate above
// 65535 never draws. A negative one always draws: the read-only entry points
// test it as 0, which passes everywhere but over a stored 0; the writing ones
// keep its low16 store, and merge_passes replays a writing body that can
// reach one through the signed manual compare (`shp_needs_manual_depth`).
@fragment
fn fs_main(input: VertexOutput) -> FragOutput {
    let index: u32 = textureLoad(source_indices, source_texel(input.uv), 0).r;
    let color = sampled_source_color(input.uv, index, input.source_palette);
    if (color.a < 0.01) {
        discard;
    }
    let z = native_candidate(input);
    if (z > 65535) {
        discard;
    }
    return zsprite_store(input, color, index, max(z, 0));
}

@fragment
fn fs_write(input: VertexOutput) -> FragOutput {
    let index: u32 = textureLoad(source_indices, source_texel(input.uv), 0).r;
    let color = sampled_source_color(input.uv, index, input.source_palette);
    if (color.a < 0.01) {
        discard;
    }
    let z = native_candidate(input);
    if (z > 65535) {
        discard;
    }
    return zsprite_store(input, color, index, z);
}

// An opaque body inside a depth-tested edit interval (terrain_depth.wgsl): the
// same store, which also restarts the pixel's shadow count. VERA scheduling
// only (terrain_draw.rs `draw_interval_tail`).
struct CountedFragOutput {
    @location(0) color: vec4f,
    @location(1) shadows: vec4f,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_counted(input: VertexOutput) -> CountedFragOutput {
    let index: u32 = textureLoad(source_indices, source_texel(input.uv), 0).r;
    let color = sampled_source_color(input.uv, index, input.source_palette);
    if (color.a < 0.01) {
        discard;
    }
    let z = native_candidate(input);
    if (z > 65535) {
        discard;
    }
    let pixel = zsprite_store(input, color, index, max(z, 0));
    var output: CountedFragOutput;
    output.color = pixel.color;
    output.depth = pixel.depth;
    return output;
}

@fragment
fn fs_counted_write(input: VertexOutput) -> CountedFragOutput {
    let index: u32 = textureLoad(source_indices, source_texel(input.uv), 0).r;
    let color = sampled_source_color(input.uv, index, input.source_palette);
    if (color.a < 0.01) {
        discard;
    }
    let z = native_candidate(input);
    if (z > 65535) {
        discard;
    }
    let pixel = zsprite_store(input, color, index, z);
    var output: CountedFragOutput;
    output.color = pixel.color;
    output.depth = pixel.depth;
    return output;
}
