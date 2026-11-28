//! VERA's HD art switch: the original picture, or the HD art pack while the
//! camera zooms in.
//!
//! VERA-only presentation extension, `[VERA] HdArt=` (off by default), the
//! Options page's HD art list and the HD art key (Page Up), which flip the same
//! setting. On, every zoom above 1 draws buildings, infantry, effects,
//! terrain and overlays from the 2x HD art pack, sampled by the ordinary
//! world blitters at the level the zoom needs (`render::hd_art`,
//! `hd_terrain`; `docs/zoom-one-pass.md`), and the smooth sidebar its HD
//! pictures (`render::sidebar_smoothing`). Voxels show their 2x pictures at
//! any zoom above 1 whatever the switch (`vxl_raster::VxlHd`). Off, nothing
//! is encoded and the picture is the original, byte for byte. Nothing here
//! reaches `sim/`, the network, a save or a replay, so clients may differ
//! without a desync.

/// The wheel zoom a match starts at with HD art on, from below
/// [`EASE_BELOW`]: the 2x art shows all its pixels there. The HD art key
/// leaves the zoom as it is.
pub(crate) const DEFAULT_ZOOM: f32 = 2.0;

/// The wheel zoom below which a match's start eases in to [`DEFAULT_ZOOM`].
pub(crate) const EASE_BELOW: f32 = 1.5;

/// `VERA20K_HD_ART=0|1|debug` replaces the profile's switch for this process,
/// so a tactical capture (which runs with a private settings folder) can take
/// before and after pictures with one build. `debug` is on with the HD art
/// tinted green and the HD terrain magenta as it is put on its twins.
/// Returns (on, debug tint).
pub(crate) fn env_override() -> Option<(bool, bool)> {
    static ENV: std::sync::OnceLock<Option<(bool, bool)>> = std::sync::OnceLock::new();
    *ENV.get_or_init(|| parse_env(&std::env::var("VERA20K_HD_ART").ok()?))
}

fn parse_env(text: &str) -> Option<(bool, bool)> {
    match text.trim() {
        "0" | "off" => Some((false, false)),
        "1" | "on" => Some((true, false)),
        "debug" => Some((true, true)),
        _ => None,
    }
}

/// `VERA20K_HD_CAPTURE_ZOOM=<percent>` holds the wheel zoom at that percent
/// (the wheel's range, 50 to 400) for this process. Tactical captures have no wheel gesture;
/// this lets them show the HD art. Presentation only: the camera's zoom never
/// reaches the simulation.
pub(crate) fn capture_zoom() -> Option<f32> {
    static ZOOM: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *ZOOM.get_or_init(|| {
        let percent: f32 = std::env::var("VERA20K_HD_CAPTURE_ZOOM")
            .ok()?
            .trim()
            .parse()
            .ok()?;
        // `app::input::camera::{MIN_ZOOM, MAX_ZOOM}`; `render` stays below `app`.
        Some((percent / 100.0).clamp(0.5, 4.0))
    })
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::parse_env;

    #[test]
    fn env_reads_off_on_and_debug_only() {
        assert_eq!(parse_env("0"), Some((false, false)));
        assert_eq!(parse_env(" 1 "), Some((true, false)));
        assert_eq!(parse_env("debug"), Some((true, true)));
        assert_eq!(parse_env("5,6,3"), None);
    }
}
