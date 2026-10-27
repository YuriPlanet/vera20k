//! Browser WebGL2 fallback helpers (see `gpu::webgl_fallback`).
//!
//! WebGL2 lacks three things the ordinary renderers use; each owner keeps its
//! own path and selects these equivalents only on the fallback device:
//! - Reinterpreting views. Native passes read and write the encoded bytes of
//!   an sRGB target through its unorm sibling view. The fallback binds the
//!   sRGB view itself; shaders re-encode what they load and decode what they
//!   store, so the stored bytes are the same.
//! - Read-only storage arrays become 2D data textures (`table_texture`).
//! - Fragment atomics and compute become ordered render-target passes in the
//!   owning renderer.
//!
//! wgpu-hal 29 GLES reports no VIEW_FORMATS, storage or compute on WebGL:
//! https://docs.rs/crate/wgpu-hal/29.0.4/source/src/gles/adapter.rs

use super::gpu::webgl_fallback;

/// Texels per row of a [`table_texture`]; within every WebGL2 size limit.
const TABLE_WIDTH: u32 = 1024;

/// `formats` on ordinary devices; none on the fallback, which cannot create
/// textures with additional view formats.
pub(crate) fn view_formats<'a>(
    device: &wgpu::Device,
    formats: &'a [wgpu::TextureFormat],
) -> &'a [wgpu::TextureFormat] {
    if webgl_fallback(device) { &[] } else { formats }
}

/// Format through which a pass reads or writes the encoded bytes of an sRGB
/// target: its unorm sibling, or the sRGB format itself on the fallback.
pub(crate) fn encoded_format(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> wgpu::TextureFormat {
    if webgl_fallback(device) {
        format
    } else {
        format.remove_srgb_suffix()
    }
}

/// The shared sRGB transfer functions from `palette_light.wgsl`, for shaders
/// that do not already include them.
pub(crate) fn srgb_wgsl() -> &'static str {
    let source = include_str!("palette_light.wgsl");
    &source[..source
        .find("fn palette_light(")
        .expect("sRGB helpers precede palette_light")]
}

/// WebGL2 uniform bindings must be a multiple of 16 bytes; the camera
/// uniforms are 40 (`CameraUniform`) or 24 (combat light) bytes. On the
/// fallback each `struct Camera` gains a trailing `vec2f`, reaching 48 or 32.
pub(crate) fn camera_source(device: &wgpu::Device, source: String) -> String {
    if webgl_fallback(device) {
        pad_camera_structs(&source)
    } else {
        source
    }
}

pub(crate) fn pad_camera_structs(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find("struct Camera {") {
        let end = start + rest[start..].find("};").expect("closed Camera struct");
        output.push_str(&rest[..end]);
        output.push_str("    fallback_pad: vec2f,\n");
        rest = &rest[end..];
    }
    output.push_str(rest);
    output
}

/// Initial uniform contents, zero-extended to the size `camera_source`
/// declares on the fallback.
pub(crate) fn uniform_bytes<'a>(
    device: &wgpu::Device,
    bytes: &'a [u8],
) -> std::borrow::Cow<'a, [u8]> {
    if webgl_fallback(device) && !bytes.len().is_multiple_of(16) {
        let mut padded = bytes.to_vec();
        padded.resize(bytes.len().next_multiple_of(16), 0);
        padded.into()
    } else {
        bytes.into()
    }
}

/// `source` with `from` replaced by `to`. The fallback edits ordinary shader
/// sources rather than keeping copies, so each edit must still apply.
pub(crate) fn patch(source: &str, from: &str, to: &str) -> String {
    assert_eq!(
        source.matches(from).count(),
        1,
        "WebGL2 shader edit no longer applies: {from}"
    );
    source.replacen(from, to, 1)
}

/// `source` for a pass whose encoded-byte views are sRGB views on the
/// fallback: applies `edits` (each must apply once) and defines
/// `fallback_load_encoded` (bytes of a loaded texel) and `fallback_store_encoded`
/// (output whose stored bytes are the given encoded value).
pub(crate) fn encoded_shader(source: &str, edits: &[(&str, &str)]) -> String {
    let mut output = String::new();
    if !source.contains("fn srgb_encode(") {
        output.push_str(srgb_wgsl());
    }
    output.push_str(
        "fn fallback_load_encoded(c: vec4f) -> vec4f { return vec4f(srgb_encode(c.rgb), c.a); }
fn fallback_store_encoded(c: vec4f) -> vec4f { return vec4f(srgb_decode(c.rgb), c.a); }
",
    );
    let mut body = source.to_owned();
    for (from, to) in edits {
        body = patch(&body, from, to);
    }
    output.push_str(&body);
    output
}

/// `source` with every `name[index]` rewritten as `function(index)`, for a
/// read-only storage array replaced by a texture lookup.
pub(crate) fn index_calls(source: &str, name: &str, function: &str) -> String {
    let opening = format!("{name}[");
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find(&opening) {
        let boundary = rest[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        output.push_str(&rest[..start]);
        rest = &rest[start + opening.len()..];
        if boundary {
            output.push_str(&opening);
            continue;
        }
        let mut depth = 1;
        let end = rest
            .char_indices()
            .find(|&(_, c)| {
                depth += match c {
                    '[' => 1,
                    ']' => -1,
                    _ => 0,
                };
                depth == 0
            })
            .map(|(index, _)| index)
            .expect("balanced storage index");
        output.push_str(function);
        output.push('(');
        output.push_str(&rest[..end]);
        output.push(')');
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    output
}

/// WGSL for the texel holding element `index` of a [`table_texture`].
pub(crate) const TABLE_WGSL: &str = "fn fallback_table_texel(index: u32) -> vec2i {
    return vec2i(i32(index % 1024u), i32(index / 1024u));
}";

/// A read-only 32-bit table as an `R32Float`/`R32Uint` data texture, read
/// with `textureLoad(table, fallback_table_texel(index), 0).r`.
pub(crate) fn table_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    format: wgpu::TextureFormat,
    words: &[u8],
) -> wgpu::TextureView {
    let texture = empty_table(device, label, format, words.len() / 4);
    write_table(queue, &texture, words);
    texture.create_view(&Default::default())
}

/// An uninitialized [`table_texture`] holding at least `capacity` words.
pub(crate) fn empty_table(
    device: &wgpu::Device,
    label: &str,
    format: wgpu::TextureFormat,
    capacity: usize,
) -> wgpu::Texture {
    assert!(matches!(
        format,
        wgpu::TextureFormat::R32Float | wgpu::TextureFormat::R32Uint
    ));
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: TABLE_WIDTH,
            height: (capacity.max(1) as u32).div_ceil(TABLE_WIDTH),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Words a table texture can hold within the device's 2D size limit.
pub(crate) fn table_capacity(device: &wgpu::Device) -> usize {
    TABLE_WIDTH as usize * device.limits().max_texture_dimension_2d as usize
}

/// Write `words` from the table's first element; whole rows are uploaded.
pub(crate) fn write_table(queue: &wgpu::Queue, texture: &wgpu::Texture, words: &[u8]) {
    assert_eq!(words.len() % 4, 0);
    let rows = ((words.len() / 4).max(1) as u32).div_ceil(TABLE_WIDTH);
    assert!(rows <= texture.height(), "table upload exceeds its texture");
    let mut texels = words.to_vec();
    texels.resize((TABLE_WIDTH * rows * 4) as usize, 0);
    queue.write_texture(
        texture.as_image_copy(),
        &texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(TABLE_WIDTH * 4),
            rows_per_image: Some(rows),
        },
        wgpu::Extent3d {
            width: TABLE_WIDTH,
            height: rows,
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_calls_rewrites_nested_indices_only_for_the_named_array() {
        let source = "let a = trig[u32(i[0])] + my_trig[1] + trig[2u];";
        assert_eq!(
            index_calls(source, "trig", "load_trig"),
            "let a = load_trig(u32(i[0])) + my_trig[1] + load_trig(2u);"
        );
    }
}
