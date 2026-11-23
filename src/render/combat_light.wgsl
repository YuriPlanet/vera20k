// Exact encoded-byte RGB565 edit used by the native persistent combat-light
// vector. The scene snapshot and target are compatible non-sRGB views of the
// same sRGB-format tactical composition resources.

struct Camera {
    screen_size: vec2f,
    camera_pos: vec2f,
    zoom: f32,
    pad: f32,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var scene: texture_2d<f32>;
@group(2) @binding(0) var native_masks: texture_2d_array<u32>;

// One pass's lights in draw order, each with a bit for every earlier light
// of the pass whose copied bounds meet its own.
struct Light {
    center: vec2i,
    surface_index: u32,
    flags: u32,
    earlier: vec2u,
    pad: vec2u,
};
@group(3) @binding(0) var<uniform> lights: array<Light, 64>;

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) light: u32,
};

// Top-left target pixel of the light's 256x128 byte surface.
fn surface_origin(light: Light) -> vec2f {
    // CoordsToClient2 has already committed its signed integer projection;
    // conversion to float happens only for camera/zoom presentation.
    let screen_center = (vec2f(light.center) - camera.camera_pos) * camera.zoom;
    // The BSurface footprint is screen-fixed. Only its projected centre follows
    // the tactical camera; zoom must not scale the 256x128 byte surface itself.
    return screen_center - vec2f(128.0, 64.0);
}

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, @builtin(instance_index) light: u32) -> VertexOutput {
    let quad = array<vec2f, 6>(
        vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0),
    );
    // One pixel beyond the surface on every side: each fragment's own surface
    // pixel decides coverage, as it does for the earlier lights it replays.
    let screen = surface_origin(lights[light]) - vec2f(1.0) + quad[vertex] * vec2f(258.0, 130.0);

    var output: VertexOutput;
    output.position = vec4f(
        screen.x / camera.screen_size.x * 2.0 - 1.0,
        1.0 - screen.y / camera.screen_size.y * 2.0,
        0.0,
        1.0,
    );
    output.light = light;
    return output;
}

// The light's mask byte at target pixel `pixel`; 0 outside its surface.
fn mask_at(light: Light, pixel: vec2f) -> u32 {
    let local = vec2i(floor(pixel - surface_origin(light)));
    if (local.x < 0 || local.y < 0 || local.x >= 256 || local.y >= 128) {
        return 0u;
    }
    return textureLoad(native_masks, local, i32(light.surface_index), 0).r;
}

fn quantize_565(encoded: vec4u) -> vec4u {
    return vec4u(encoded.r & 0xf8u, encoded.g & 0xfcu, encoded.b & 0xf8u, encoded.a);
}

fn transform_channel(value: u32, mask: u32, flags: u32, disable_bit: u32) -> u32 {
    if ((flags & 1u) != 0u) {
        return (value * (256u - mask)) >> 8u;
    }
    if ((flags & disable_bit) != 0u) {
        return value;
    }
    return min((value * (256u + mask)) >> 8u, 255u);
}

fn lit(destination: vec4u, mask: u32, flags: u32) -> vec4u {
    return quantize_565(vec4u(
        transform_channel(destination.r, mask, flags, 2u),
        transform_channel(destination.g, mask, flags, 4u),
        transform_channel(destination.b, mask, flags, 8u),
        destination.a,
    ));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4f {
    let light = lights[input.light];
    let mask = mask_at(light, input.position.xy);
    if (mask == 0u) {
        discard;
    }
    let destination = quantize_565(vec4u(round(textureLoad(scene, vec2i(input.position.xy), 0) * 255.0)));
    // The scratch holds this pixel as the pass found it. Each earlier light
    // of the pass that reaches it already wrote its edit here and will be
    // overwritten, so apply theirs first, in order, then this one.
    var transformed = destination;
    for (var earlier = 0u; earlier < input.light; earlier++) {
        let bits = select(light.earlier.x, light.earlier.y, earlier >= 32u);
        if (((bits >> (earlier % 32u)) & 1u) == 0u) {
            continue;
        }
        let other = lights[earlier];
        let other_mask = mask_at(other, input.position.xy);
        if (other_mask != 0u) {
            transformed = lit(transformed, other_mask, other.flags);
        }
    }
    transformed = lit(transformed, mask, light.flags);
    return vec4f(transformed) / 255.0;
}
