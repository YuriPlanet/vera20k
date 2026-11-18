// VERA-only, experimental model art (`Model=`, docs/picture-model-art.md):
// one mesh primitive of a declared model, drawn into the frame's model page
// (render::model_draw). The page is an RGBA source page of the sprite atlas
// with a palette-index plane, which the SHP blitters read as picture pages:
// index 1 for a lit pixel, 240 for one light leaves alone, 0 for a hole.

struct Draw {
    // Model space to the page's clip space: the part's voxel draw matrix,
    // the model's scale and axes, and the image's place on the page.
    clip: mat4x4<f32>,
    // Model space to the voxel camera's space for normals (inverse transpose).
    normal0: vec4<f32>,
    normal1: vec4<f32>,
    normal2: vec4<f32>,
    base_color: vec4<f32>,
    // rgb: the emissive factor.
    emissive: vec4<f32>,
    // xyz: the voxel world light in the camera's space; w: the Mask cutoff.
    light: vec4<f32>,
    // z: alpha mode (0 opaque, 1 mask, 2 blend); w: 1 when `ramp` is the
    // owner's house colour.
    flags: vec4<u32>,
    // The owner's ramp, palette indices 16..31, linear colour.
    ramp: array<vec4<f32>, 16>,
}

@group(0) @binding(0) var<uniform> draw: Draw;
@group(1) @binding(0) var base_texture: texture_2d<f32>;
@group(1) @binding(1) var emissive_texture: texture_2d<f32>;
@group(1) @binding(2) var mask_texture: texture_2d<f32>;
@group(1) @binding(3) var model_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = draw.clip * vec4<f32>(vertex.position, 1.0);
    let normal = mat3x3<f32>(draw.normal0.xyz, draw.normal1.xyz, draw.normal2.xyz);
    out.normal = normal * vertex.normal;
    out.uv = vertex.uv;
    out.color = vertex.color;
    return out;
}

struct FragmentOut {
    @location(0) color: vec4<f32>,
    @location(1) index: u32,
}

// A linear channel as an sRGB byte value, 0..255.
fn srgb_byte(linear: f32) -> f32 {
    let c = clamp(linear, 0.0, 1.0);
    let encoded = select(1.055 * pow(c, 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0031308);
    return encoded * 255.0;
}

@fragment
fn fs_main(in: VertexOut) -> FragmentOut {
    // Every texture is sampled at level 0: the pages have no mips, and the
    // samples stay outside the branches below.
    let base = draw.base_color * in.color * textureSampleLevel(base_texture, model_sampler, in.uv, 0.0);
    let mask = textureSampleLevel(mask_texture, model_sampler, in.uv, 0.0);
    let emissive = textureSampleLevel(emissive_texture, model_sampler, in.uv, 0.0).rgb * draw.emissive.rgb;
    // A pixel at least half covered is drawn, as a picture's is; Mask uses
    // its own cutoff.
    let cutoff = select(0.5, draw.light.w, draw.flags.z == 1u);
    if draw.flags.z != 0u && base.a < cutoff {
        discard;
    }
    var color = base.rgb;
    var lit = true;
    if mask.r >= 0.5 {
        // House colour by brightness, as render::picture_atlas takes it:
        // the largest channel picks the ramp step, 255 the lightest, every
        // 17 levels darker one step down.
        if draw.flags.w != 0u {
            let brightness = srgb_byte(max(max(color.r, color.g), color.b));
            let step = min(u32((255.0 - brightness) / 17.0), 15u);
            color = draw.ramp[step].rgb;
        }
    } else if mask.g >= 0.5 {
        lit = false;
    }
    // Emissive parts are unlit.
    if max(max(emissive.r, emissive.g), emissive.b) >= 0.5 {
        color = emissive;
        lit = false;
    }
    var out: FragmentOut;
    if lit {
        // The voxel light (render::vxl_normals::blinn_phong_pages): diffuse
        // plus the Schlick term of the halfway vector to the +Z viewer at
        // strength 3, where a page of 16 is the colour as it is.
        let size = max(length(in.normal), 1e-6);
        let n = in.normal / size;
        let l = draw.light.xyz;
        let diffuse = max(dot(n, l), 0.0);
        let half_vector = normalize(l + vec3<f32>(0.0, 0.0, 1.0));
        let h = dot(half_vector, n);
        let specular = max(h / (3.0 - 2.0 * h), 0.0);
        color = min(color * min(diffuse + specular, 2.0), vec3<f32>(1.0));
        out.index = 1u;
    } else {
        out.index = 240u;
    }
    out.color = vec4<f32>(color, 1.0);
    return out;
}
