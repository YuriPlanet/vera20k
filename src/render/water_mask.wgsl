// The post-process water mask: one white iso diamond per visible water cell.
//
// The vertex buffer is rebuilt every frame in frame pixels (the same rounded
// camera subtraction the sprite shader applies); the fragment writes 1.0 into
// an R8 target. The post-process pass samples it with `textureLoad`, so a texel
// is water exactly when the engine marked that cell as `LandType::Water`; no
// colour or depth heuristic guesses it.

struct MaskParams {
    size: vec2f,
};

@group(0) @binding(0) var<uniform> params: MaskParams;

@vertex
fn vs_main(@location(0) position: vec2f) -> @builtin(position) vec4f {
    let clip = vec2f(
        position.x / params.size.x * 2.0 - 1.0,
        1.0 - position.y / params.size.y * 2.0,
    );
    return vec4f(clip, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4f {
    return vec4f(1.0, 0.0, 0.0, 0.0);
}
