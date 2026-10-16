//! Assembly of shared tactical palette and stored-depth shader mechanisms.

pub(crate) fn source(body: &str) -> String {
    super::palette_light::shader_source(&format!(
        "{}\n{}\n{}",
        include_str!("native_z.wgsl"),
        include_str!("native_packed.wgsl"),
        body
    ))
}

/// World source blitters use the shared Camera + A binding. Packed destination
/// resolve and LineTrail keep their own binding/operation classes and include
/// only palette math and stored-Z helpers through `source` above.
pub(crate) fn world_source(body: &str) -> String {
    source(&format!("{}\n{}", include_str!("tactical_a.wgsl"), body))
}

/// The source page an SHP-family blitter reads, RGBA or palette-indexed.
pub(crate) const SPRITE_SOURCE: &str = include_str!("sprite_source.wgsl");

/// World blitters that read a source page: the batch and Z-tested SHP shaders
/// and the terrain passes built on the SHP projection.
pub(crate) fn sprite_source(body: &str) -> String {
    world_source(&format!("{SPRITE_SOURCE}\n{body}"))
}
