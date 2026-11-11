// Tactical A composition into the shared R8 plane: the cell loop
// (6D7312..6D7532 -> 4801F0 -> 47EFE0 copy / 47F250 fog blend) and then
// AlphaShapeClass::DrawAll_NoMask 421350. Integer math only; the plane holds
// each native byte as byte/255, which every unorm conversion round-trips.

fn target_clip(pixel: vec2f, size: vec2f) -> vec4f {
    return vec4f(pixel.x / size.x * 2.0 - 1.0, 1.0 - pixel.y / size.y * 2.0, 0.0, 1.0);
}

// The quad's corner for a four-vertex strip.
fn strip_corner(vertex: u32) -> vec2f {
    return vec2f(f32(vertex & 1u), f32(vertex >> 1u));
}

struct CellParams { target_size: vec2f, canvas: vec2f };
@group(0) @binding(0) var<uniform> cell: CellParams;
@group(0) @binding(1) var frames: texture_2d<u32>;

struct CellOutput {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) origin: vec2i,
    @location(1) @interpolate(flat) atlas: vec2i,
};

@vertex
fn vs_cell(@builtin(vertex_index) vertex: u32, @location(0) origin: vec2i,
           @location(1) atlas: vec2u) -> CellOutput {
    var out: CellOutput;
    out.position = target_clip(vec2f(origin) + strip_corner(vertex) * cell.canvas, cell.target_size);
    out.origin = origin;
    out.atlas = vec2i(atlas);
    return out;
}

fn frame_byte(input: CellOutput) -> u32 {
    let local = vec2i(floor(input.position.xy)) - input.origin;
    return textureLoad(frames, input.atlas + local, 0).r;
}

// 47EFE0 stores every source byte except the transparent FE.
@fragment
fn fs_copy(input: CellOutput) -> @location(0) vec4f {
    let byte = frame_byte(input);
    if byte == 254u { discard; }
    return vec4f(f32(byte) / 255.0, 0.0, 0.0, 1.0);
}

// 47F403..47F43D / 47F4A2..47F4DE: bytes above 127 are skipped; the rest
// store max(A + byte - 127, 0). The pipeline's reverse-subtract blend stores
// A - (127 - byte), which the unorm target clamps at zero.
@fragment
fn fs_blend(input: CellOutput) -> @location(0) vec4f {
    let byte = frame_byte(input);
    if byte > 127u { discard; }
    return vec4f(f32(127u - byte) / 255.0, 0.0, 0.0, 0.0);
}

// Distinct binding numbers keep one module free of shared resource slots.
struct GlowParams { target_size: vec2f, pad: vec2f };
@group(0) @binding(2) var<uniform> glow: GlowParams;
@group(0) @binding(3) var plane: texture_2d<f32>;
@group(1) @binding(0) var glow_image: texture_2d<u32>;

struct GlowOutput {
    @builtin(position) position: vec4f,
    // Plane pixel minus scratch pixel, and the plane pixel of image texel 0.
    @location(0) @interpolate(flat) plane_offset: vec2i,
    @location(1) @interpolate(flat) image_origin: vec2i,
};

@vertex
fn vs_glow(@builtin(vertex_index) vertex: u32, @location(0) scratch: vec2i,
           @location(1) size: vec2i, @location(2) plane_origin: vec2i,
           @location(3) image_origin: vec2i) -> GlowOutput {
    var out: GlowOutput;
    out.position = target_clip(vec2f(scratch) + strip_corner(vertex) * vec2f(size), glow.target_size);
    out.plane_offset = plane_origin - scratch;
    out.image_origin = image_origin;
    return out;
}

// Table 0x0088A118 (built at 0x00420A71): min(255, byte * A / 127). The
// scratch holds the result until it is copied back over the same rectangle.
@fragment
fn fs_glow(input: GlowOutput) -> @location(0) vec4f {
    let pixel = vec2i(floor(input.position.xy)) + input.plane_offset;
    let a = u32(round(textureLoad(plane, pixel, 0).r * 255.0));
    let byte = textureLoad(glow_image, pixel - input.image_origin, 0).r;
    return vec4f(f32(min(byte * a / 127u, 255u)) / 255.0, 0.0, 0.0, 1.0);
}
