// VERA-only, experimental model art (`Model=`, docs/picture-model-art.md):
// one mesh primitive of a declared model, drawn into the frame's model page
// (render::model_draw). The page is an RGBA source page of the sprite atlas
// with a palette-index plane, which the SHP blitters read as picture pages:
// index 1 for a lit pixel, 240 for one light leaves alone, 0 for a hole.

struct Draw {
    // Model space to the page's clip space: the part's voxel draw matrix,
    // the model's scale and axes, and the image's place on the page.
    clip: mat4x4<f32>,
    // Model space to the voxel camera's space, anchor-relative: the space the
    // normals are in, for a normal map's tangent frame.
    to_camera: mat4x4<f32>,
    // Model space to the voxel camera's space for normals (inverse transpose).
    normal0: vec4<f32>,
    normal1: vec4<f32>,
    normal2: vec4<f32>,
    base_color: vec4<f32>,
    // rgb: the emissive factor.
    emissive: vec4<f32>,
    // xyz: the voxel world light in the camera's space; w: the Mask cutoff.
    light: vec4<f32>,
    // x: 1 when `normal_texture` is a normal map; y: 1 for the diffuse
    // light of a projectile or debris piece (0x00753C80), 0 for a unit's;
    // z: alpha mode (0 opaque, 1 mask, 2 blend); w: 1 when `ramp` is the
    // owner's house colour.
    flags: vec4<u32>,
    // The owner's ramp, palette indices 16..31, linear colour.
    ramp: array<vec4<f32>, 16>,
    // xyz: the voxel draw's own +Z in the camera's space, the viewer of the
    // voxel light's Schlick term (render::vxl_normals::blinn_phong_pages
    // lights in the draw's model space, viewer +Z there).
    view: vec4<f32>,
    // The VPL's brightness per light page 0..31, four to a vector: how much
    // each page scales a palette colour's bytes (page 8 is about 1).
    pages: array<vec4<f32>, 8>,
}

@group(0) @binding(0) var<uniform> draw: Draw;
@group(1) @binding(0) var base_texture: texture_2d<f32>;
@group(1) @binding(1) var emissive_texture: texture_2d<f32>;
@group(1) @binding(2) var mask_texture: texture_2d<f32>;
@group(1) @binding(3) var model_sampler: sampler;
// A tangent-space normal map (glTF convention, +Y up), linear.
@group(1) @binding(4) var normal_texture: texture_2d<f32>;

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
    @location(3) camera: vec3<f32>,
}

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = draw.clip * vec4<f32>(vertex.position, 1.0);
    let normal = mat3x3<f32>(draw.normal0.xyz, draw.normal1.xyz, draw.normal2.xyz);
    out.normal = normal * vertex.normal;
    out.uv = vertex.uv;
    out.color = vertex.color;
    out.camera = (draw.to_camera * vec4<f32>(vertex.position, 1.0)).xyz;
    return out;
}

struct FragmentOut {
    @location(0) color: vec4<f32>,
    @location(1) index: u32,
}

// `n` bent by the tangent-space normal `t` in the cotangent frame the screen
// derivatives of position (`dp1`, `dp2`) and UV (`duv1`, `duv2`) give, so no
// tangent attribute is needed. The frame's second axis runs along +V, and a
// glTF normal map's +Y runs along -V: its y is turned.
fn bent_normal(n: vec3<f32>, t: vec3<f32>, dp1: vec3<f32>, dp2: vec3<f32>, duv1: vec2<f32>, duv2: vec2<f32>) -> vec3<f32> {
    let dp2_perp = cross(dp2, n);
    let dp1_perp = cross(n, dp1);
    let tangent = dp2_perp * duv1.x + dp1_perp * duv2.x;
    let bitangent = dp2_perp * duv1.y + dp1_perp * duv2.y;
    let size = max(dot(tangent, tangent), dot(bitangent, bitangent));
    if size <= 0.0 {
        return n;
    }
    let scale = inverseSqrt(size);
    let bent = mat3x3<f32>(tangent * scale, bitangent * scale, n) * vec3<f32>(t.x, -t.y, t.z);
    let length_bent = length(bent);
    return select(n, bent / length_bent, length_bent > 1e-6);
}

// A linear channel as an sRGB byte value, 0..255.
fn srgb_byte(linear: f32) -> f32 {
    let c = clamp(linear, 0.0, 1.0);
    let encoded = select(1.055 * pow(c, 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0031308);
    return encoded * 255.0;
}

fn srgb_encode(linear: vec3<f32>) -> vec3<f32> {
    let c = clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

fn srgb_decode(encoded: vec3<f32>) -> vec3<f32> {
    let c = clamp(encoded, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// The VPL brightness of light page `page` (0..31).
fn vpl_page(page: u32) -> f32 {
    return draw.pages[page / 4u][page % 4u];
}

@fragment
fn fs_main(in: VertexOut) -> FragmentOut {
    // Derivatives before any branch, in uniform control flow.
    let dp1 = dpdx(in.camera);
    let dp2 = dpdy(in.camera);
    let duv1 = dpdx(in.uv);
    let duv2 = dpdy(in.uv);
    // Textures carry mipmaps (model_draw.rs `rgba_texture`), sampled at the
    // level the screen derivatives pick, so a texture shrunk to a few page
    // pixels averages instead of breaking into noise. The samples stay in
    // uniform control flow, before the branches below.
    let normal_sample = textureSample(normal_texture, model_sampler, in.uv).xyz * 2.0 - vec3<f32>(1.0);
    let base = draw.base_color * in.color * textureSample(base_texture, model_sampler, in.uv);
    let mask = textureSample(mask_texture, model_sampler, in.uv);
    let emissive = textureSample(emissive_texture, model_sampler, in.uv).rgb * draw.emissive.rgb;
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
        // The voxel light (render::vxl_normals::blinn_phong_pages): the page
        // is 16 times diffuse plus the Schlick term of the halfway vector to
        // the viewer at strength 3, the viewer being the draw's own +Z. A
        // projectile or debris piece takes the diffuse term alone
        // (render::vxl_normals::diffuse_pages, 0x00758670: page
        // 16 * dot(n, l), 0 where the dot is negative). The page then lights
        // the colour as the VPL lights a palette colour: by that page's
        // brightness, on the colour's encoded bytes, where page 0 still
        // keeps about half of it and page 8 all of it. Neither quantises to
        // whole pages: a model's normals are not the 244 table normals.
        let size = max(length(in.normal), 1e-6);
        var n = in.normal / size;
        if draw.flags.x != 0u {
            n = bent_normal(n, normal_sample, dp1, dp2, duv1, duv2);
        }
        let l = draw.light.xyz;
        let diffuse = max(dot(n, l), 0.0);
        let half_vector = normalize(l + draw.view.xyz);
        let h = dot(half_vector, n);
        let specular = select(max(h / (3.0 - 2.0 * h), 0.0), 0.0, draw.flags.y != 0u);
        let page = clamp((diffuse + specular) * 16.0, 0.0, 31.0);
        let low = u32(floor(page));
        let brightness = mix(vpl_page(low), vpl_page(min(low + 1u, 31u)), page - f32(low));
        color = srgb_decode(srgb_encode(color) * brightness);
        out.index = 1u;
    } else {
        out.index = 240u;
    }
    out.color = vec4<f32>(color, 1.0);
    return out;
}
