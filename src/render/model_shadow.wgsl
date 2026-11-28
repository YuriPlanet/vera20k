// VERA-only, experimental model art (`Model=`, docs/picture-model-art.md):
// one mesh primitive of a declared model's shadow, drawn into the frame's
// model page (render::model_draw). The block's matrix already flattens the
// mesh onto its bottom face and projects it as 0x00753F90 projects a VXL
// section's bottom-face quad; every covered pixel is a shadow pixel. The SHP
// shadow edit reads only the index plane, nonzero where shadowed.

struct ShadowDraw {
    // Model space to the page's clip space (the first matrix of
    // model_draw.wgsl's `Draw` block).
    clip: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> draw: ShadowDraw;

@vertex
fn vs_shadow(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return draw.clip * vec4<f32>(position, 1.0);
}

struct ShadowOut {
    @location(0) color: vec4<f32>,
    @location(1) index: u32,
}

@fragment
fn fs_shadow() -> ShadowOut {
    var out: ShadowOut;
    out.color = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    out.index = 1u;
    return out;
}
