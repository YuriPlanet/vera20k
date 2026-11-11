// The radar's primary surface (RadarClass::Update 0x00656EC0): each aperture
// pixel's generated terrain under its cell's shroud and fog (RenderCellPixel
// 0x00655C50), then the object, radar-event and beacon stores in update
// order, the last store winning. Composite texels hold a pixel's RGBA bytes
// packed little-endian, so VERA's HD radar compares them exactly.

struct Params {
    // Aperture pixel of generated-surface pixel (0, 0), and the surface size.
    offset: vec2i,
    size: vec2i,
    // VERA's HD radar: its surface's origin in the HD aperture, and its size.
    hd_origin: vec2i,
    hd_size: vec2i,
    plane_size: vec2u,
    hd_scale: u32,
    // 0: every cell revealed; 1: the viewer's cell flags; 2: no plane.
    visibility: u32,
    // The plane's flag bits (sim::vision).
    revealed: u32,
    gap_covered: u32,
    gap_fog: u32,
    pad: u32,
};
@group(0) @binding(0) var<uniform> params: Params;
// Generated surface: R5G6B5 words, and each pixel's cell as rx | ry << 16.
@group(0) @binding(1) var terrain: texture_2d<u32>;
@group(0) @binding(2) var cells: texture_2d<u32>;
@group(0) @binding(3) var plane: texture_2d<u32>;
@group(0) @binding(4) var hd_terrain: texture_2d<u32>;
@group(0) @binding(5) var hd_cells: texture_2d<u32>;
@group(1) @binding(0) var composite: texture_2d<u32>;

// COLOR_SHROUD, opaque black.
const SHROUD: u32 = 0xff000000u;

// RGB565.unpack_rgb8 zero-fills each channel; the pixel is opaque.
fn rgb565_bytes(word: u32) -> u32 {
    let red = ((word >> 11u) & 31u) << 3u;
    let green = ((word >> 5u) & 63u) << 2u;
    let blue = (word & 31u) << 3u;
    return red | (green << 8u) | (blue << 16u) | 0xff000000u;
}

// What a surface pixel shows for its cell, in RenderCellPixel's order: a
// hostile gap generator's cover first (black wins over half-bright), then a
// friendly one's half-bright fog, then whether the viewer revealed the cell.
// Half-bright halves each R5G6B5 channel. A cell off the plane has no flags.
fn cell_pixel(word: u32, cell: u32) -> u32 {
    if params.visibility == 0u {
        return rgb565_bytes(word);
    }
    if params.visibility != 1u {
        return SHROUD;
    }
    let at = vec2u(cell & 0xffffu, cell >> 16u);
    var flags = 0u;
    if all(at < params.plane_size) {
        flags = textureLoad(plane, vec2i(at), 0).r;
    }
    if (flags & params.gap_covered) != 0u {
        return SHROUD;
    }
    if (flags & params.gap_fog) != 0u {
        return rgb565_bytes((word >> 1u) & 0x7befu);
    }
    if (flags & params.revealed) != 0u {
        return rgb565_bytes(word);
    }
    return SHROUD;
}

// A native aperture pixel before the stores: the shroud fill around the
// centred generated surface.
fn native_terrain(pixel: vec2i) -> u32 {
    let local = pixel - params.offset;
    if any(local < vec2i(0)) || any(local >= params.size) {
        return SHROUD;
    }
    return cell_pixel(textureLoad(terrain, local, 0).r, textureLoad(cells, local, 0).r);
}

// One triangle over the whole target.
@vertex
fn vs_fullscreen(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4f {
    let corner = vec2f(f32((vertex << 1u) & 2u), f32(vertex & 2u));
    return vec4f(corner.x * 2.0 - 1.0, 1.0 - corner.y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_terrain(@builtin(position) position: vec4f) -> @location(0) u32 {
    return native_terrain(vec2i(position.xy));
}

struct StoreOutput {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) color: u32,
};

// One aperture pixel per store: x | y << 16, then its bytes.
@vertex
fn vs_store(@builtin(vertex_index) vertex: u32, @location(0) store: vec2u) -> StoreOutput {
    let pixel = vec2f(f32(store.x & 0xffffu), f32(store.x >> 16u));
    let at = (pixel + vec2f(f32(vertex & 1u), f32(vertex >> 1u))) / APERTURE;
    var out: StoreOutput;
    out.position = vec4f(at.x * 2.0 - 1.0, 1.0 - at.y * 2.0, 0.0, 1.0);
    out.color = store.y;
    return out;
}

@fragment
fn fs_store(input: StoreOutput) -> @location(0) u32 {
    return input.color;
}

// Bytes into an sRGB target, which stores them unchanged.
fn presented(bytes: u32) -> vec4f {
    let rgba = vec4f(vec4u(bytes, bytes >> 8u, bytes >> 16u, bytes >> 24u) & vec4u(255u)) / 255.0;
    return vec4f(srgb_decode(rgba.rgb), rgba.a);
}

@fragment
fn fs_present(@builtin(position) position: vec4f) -> @location(0) vec4f {
    return presented(textureLoad(composite, vec2i(position.xy), 0).r);
}

// VERA's HD radar: a native pixel a store changed covers its whole block;
// elsewhere each HD pixel shows the HD terrain under its own cell's shroud
// and fog, with the shroud fill around the HD surface.
@fragment
fn fs_hd(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let pixel = vec2i(position.xy);
    let native = pixel / i32(params.hd_scale);
    let stored = textureLoad(composite, native, 0).r;
    if stored != native_terrain(native) {
        return presented(stored);
    }
    let local = pixel - params.hd_origin;
    if any(local < vec2i(0)) || any(local >= params.hd_size) {
        return presented(SHROUD);
    }
    return presented(cell_pixel(textureLoad(hd_terrain, local, 0).r,
                                textureLoad(hd_cells, local, 0).r));
}
