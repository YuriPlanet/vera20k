// Encoded-byte RGB565 presentation for the stock main-menu shell.

@group(0) @binding(0) var source: texture_2d<f32>;

@group(0) @binding(1)
var<storage, read> codebook: array<u32>;

// Native 16-bit surface operations applied to RGB565 unit values before the
// presentation codebook (see `SurfaceEffects`).
struct SurfaceEffects {
    fade_rows: u32,
    height: u32,
    _pad0: u32,
    _pad1: u32,
};

@group(0) @binding(2)
var<uniform> effects: SurfaceEffects;

// Show_Credits 0x004C3E30 band fade: row r from the edge keeps floor(v*f/256)
// with f = 255 - min(256 - 8r, 255) (row 0 black, row r >= 1 f = 8r - 1).
fn credits_fade_factor(row: u32) -> u32 {
    return 255u - min(256u - 8u * row, 255u);
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
) -> @builtin(position) vec4f {
    let x = f32((vertex_index << 1u) & 2u);
    let y = f32(vertex_index & 2u);
    return vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_main(
    @builtin(position) position: vec4f,
) -> @location(0) vec4f {
    let pixel = vec2i(position.xy);
    let encoded = textureLoad(source, pixel, 0);
    let encoded_bytes = vec4u(round(encoded * 255.0));
    var units = vec3u(encoded_bytes.r >> 3u, encoded_bytes.g >> 2u, encoded_bytes.b >> 3u);
    if (effects.fade_rows > 0u) {
        let y = u32(pixel.y);
        var row = effects.fade_rows;
        if (y < effects.fade_rows) {
            row = y;
        } else if (y + effects.fade_rows >= effects.height) {
            row = effects.height - 1u - y;
        }
        if (row < effects.fade_rows) {
            units = (units * credits_fade_factor(row)) >> vec3u(8u);
        }
    }
    let red = codebook[units.r];
    let green = codebook[32u + units.g];
    let blue = codebook[units.b];
    let presented = vec4u(red, green, blue, encoded_bytes.a);
    return vec4f(presented) / 255.0;
}
