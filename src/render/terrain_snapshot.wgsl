// Read the current attachments while neither is attached for writing.
// The color binding is explicitly non-sRGB: these are encoded display bytes.
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var live_depth: texture_depth_2d;

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4f {
    let p = array<vec2f, 3>(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    return vec4f(p[vertex], 0.0, 1.0);
}

struct Snapshot {
    @location(0) word: u32,
    @location(1) depth: f32,
};
fn snapshot_at(position: vec4f) -> Snapshot {
    let p = vec2i(position.xy);
    let rgb = vec3u(round(textureLoad(scene, p, 0).rgb * 255.0));
    var output: Snapshot;
    output.word = native_rgb565_word(rgb);
    output.depth = textureLoad(live_depth, p, 0);
    return output;
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> Snapshot { return snapshot_at(position); }

// Read-only span scratch is derived only from this snapshot and admitted source
// fragments. Every covered pixel is reset before any body/shadow atomics run.
struct ReadOnlyPixel { body: atomic<u32>, shadows: atomic<u32> };
@group(1) @binding(0) var<storage, read_write> read_only_pixels: array<ReadOnlyPixel>;
@fragment
fn fs_read_only(@builtin(position) position: vec4f) -> Snapshot {
    let p = vec2u(position.xy);
    let width = textureDimensions(scene).x;
    let rows = arrayLength(&read_only_pixels) / width;
    let index = (p.y % rows) * width + p.x;
    atomicStore(&read_only_pixels[index].body, 0u);
    atomicStore(&read_only_pixels[index].shadows, 0u);
    return snapshot_at(position);
}
