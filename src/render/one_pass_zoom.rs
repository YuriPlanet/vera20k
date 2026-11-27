//! VERA-only, experimental (`docs/zoom-one-pass.md`): the battlefield drawn
//! once, straight at the zoomed resolution, instead of at native pixels and
//! magnified.

/// `VERA_ZOOM_ONE_PASS=1` turns the experiment on for this process.
pub(crate) fn switched_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("VERA_ZOOM_ONE_PASS").is_ok_and(|value| value.trim() == "1"))
}

/// Whether this frame draws the battlefield in one pass at `zoom` (the wheel
/// zoom): the switch on, zoomed in, the Options zoom at 100% (`window_copy`
/// false) and VERA's HD art off.
pub(crate) fn active(zoom: f32, window_copy: bool, hd_art_on: bool) -> bool {
    switched_on() && zoom > 1.0 && !window_copy && !hd_art_on
}
