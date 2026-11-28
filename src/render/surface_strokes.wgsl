// The GPU form of `surface_line::SurfaceStroke`: XSurface7BA610's solid
// walk, DSurface4C0750 / XSurface7BA8C0's patterned walk and FillRect's
// rectangle, one point per pixel the walk visits (one zoom-sized quad when the
// world camera is zoomed), 64 to an instance
// (`batch::StrokePiece`). Opaque stores: no Z read or write, and A only for
// a gated stroke's pass.
//
// `surface_line::walk_line`'s strict-error step in closed form: pixel k of a
// line whose major length is M and minor length m is k along the major axis
// and (2mk + M - 1) / 2M along the minor. A 45-degree line steps Y, as the
// walk's `dx > dy` test does. The rally and action-line GPU tests compare the
// result with native pixels.

struct Camera {
    screen_size: vec2f,
    camera_pos: vec2f,
    zoom: f32,
    world_origin_y: f32,
    world_height: f32,
    pad0: f32,
    native_z_origin_y: f32,
    native_z_pan: f32,
};
@group(0) @binding(0) var<uniform> camera: Camera;

// `SurfaceStroke` and the piece's first pixel.
struct Piece {
    @location(0) a: vec2i,
    @location(1) b: vec2i,
    @location(2) word: u32,
    @location(3) style: u32,
    @location(4) first: u32,
};

struct Store {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) word: u32,
    @location(1) @interpolate(flat) gate: u32,
};

const STROKE_BACKWARDS: u32 = 1048576u; // 1 << 20
const STROKE_RECT: u32 = 8388608u; // 1 << 23

// Pixel `k` of `piece`'s walk, and whether it is lit (inside the stroke and
// on in its pattern).
struct Walked {
    lit: bool,
    pixel: vec2i,
};

fn walked_pixel(piece: Piece, k: u32) -> Walked {
    var out = Walked(false, piece.a);
    if (piece.style & STROKE_RECT) != 0u {
        let width = u32(piece.b.x);
        if k >= width * u32(piece.b.y) {
            return out;
        }
        out.pixel = piece.a + vec2i(i32(k % width), i32(k / width));
        out.lit = true;
        return out;
    }
    let dx = piece.b.x - piece.a.x;
    let dy = abs(piece.b.y - piece.a.y);
    let major = max(dx, dy);
    let minor = min(dx, dy);
    var count = major;
    if dx == 0 || dy == 0 {
        count = major + 1;
    }
    if i32(k) >= count {
        return out;
    }
    let start = (piece.style >> 16u) & 15u;
    var phase = (start + k) & 15u;
    if (piece.style & STROKE_BACKWARDS) != 0u {
        phase = (start + 16u - (k & 15u)) & 15u;
    }
    if (piece.style & (1u << phase)) == 0u {
        return out;
    }
    var across = 0;
    if major > 0 {
        across = (2 * minor * i32(k) + major - 1) / (2 * major);
    }
    var y_step = 1;
    if piece.a.y > piece.b.y {
        y_step = -1;
    }
    if dx > dy {
        out.pixel = piece.a + vec2i(i32(k), y_step * across);
    } else {
        out.pixel = piece.a + vec2i(across, y_step * i32(k));
    }
    out.lit = true;
    return out;
}

fn target_clip(at: vec2f) -> vec4f {
    return vec4f(
        at.x / camera.screen_size.x * 2.0 - 1.0,
        1.0 - at.y / camera.screen_size.y * 2.0,
        0.0,
        1.0,
    );
}

// Zoom 1 (the world pass at native pixels, `set_native_world_resolution`, or
// an unzoomed frame): one point per walked pixel, the point's centre its
// pixel's under the rounded camera.
@vertex
fn vs_stroke(@builtin(vertex_index) index: u32, piece: Piece) -> Store {
    var out: Store;
    out.word = piece.word;
    out.gate = (piece.style >> 21u) & 3u;
    // A pixel past the stroke's end, or one its pattern leaves unlit, goes
    // off the target.
    out.position = vec4f(-2.0, -2.0, 0.0, 1.0);
    let walked = walked_pixel(piece, piece.first + index);
    if !walked.lit {
        return out;
    }
    let at = vec2f(walked.pixel) - camera.camera_pos + vec2f(0.5);
    out.position = target_clip(at);
    return out;
}

// Zoom != 1 (`docs/zoom-one-pass.md`, the battlefield drawn once at the
// wheel zoom): each walked pixel is a two-triangle quad over its whole
// `zoom`-sized block, snapped as the world blitters snap
// (`floor(pos - camera_pos + 0.5) * zoom`). Six vertices per pixel.
@vertex
fn vs_stroke_block(@builtin(vertex_index) index: u32, piece: Piece) -> Store {
    var out: Store;
    out.word = piece.word;
    out.gate = (piece.style >> 21u) & 3u;
    out.position = vec4f(-2.0, -2.0, 0.0, 1.0);
    let walked = walked_pixel(piece, piece.first + index / 6u);
    if !walked.lit {
        return out;
    }
    let corner_index = index % 6u;
    var corner = vec2f(0.0, 0.0);
    if corner_index == 1u || corner_index == 4u {
        corner = vec2f(1.0, 0.0);
    } else if corner_index == 2u || corner_index == 3u {
        corner = vec2f(0.0, 1.0);
    } else if corner_index == 5u {
        corner = vec2f(1.0, 1.0);
    }
    let block = floor(vec2f(walked.pixel) - camera.camera_pos + vec2f(0.5));
    out.position = target_clip(world_block_corner(block + corner));
    return out;
}

// The gate keeps a pixel where the live A is nonzero (1) or zero (2); the
// store is the word as the retail surface presents it. `tactical_a_at`
// divides by the zoom, so a block's fragments all test its world pixel's A.
@fragment
fn fs_stroke(input: Store) -> @location(0) vec4f {
    let a = tactical_a_at(input.position.xy);
    if (input.gate == 1u && a == 0u) || (input.gate == 2u && a != 0u) {
        discard;
    }
    return vec4f(retail_linear(input.word), 1.0);
}
