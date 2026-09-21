//! Render-only overlay asset-name helpers: SHP filename candidates and the
//! debug/render overlay-name resolution over a rules `OverlayTypeRegistry`.
//! Split out of `map::overlay_types` (F04); presentation-only — no sim or map
//! consumer may depend on the env-var debug remaps here.

use std::borrow::Cow;
use std::sync::OnceLock;

use crate::rules::overlay_types::OverlayTypeRegistry;

/// Generate candidate SHP filenames for an overlay name.
///
/// Returns a list of filenames to try in order, using the theater extension first
/// (e.g., for temperate: `.tem`), then the generic `.shp` extension.
/// Both lowercase and original case are tried.
pub fn overlay_shp_candidates(name: &str, theater_ext: &str) -> Vec<String> {
    let lower: String = name.to_lowercase();
    vec![
        format!("{}.{}", lower, theater_ext),
        format!("{}.shp", lower),
        format!("{}.{}", name, theater_ext),
        format!("{}.shp", name),
    ]
}

/// Optional debug remap for problematic resource overlays.
///
/// When `RA2_FORCE_TIB3_TO_TIB01=1`, remap `TIB3_20` to `TIB01`.
/// This is a temporary diagnostic switch to isolate rules/mapping issues.
pub fn remap_overlay_name_for_debug<'a>(name: &'a str) -> Cow<'a, str> {
    static FORCE_TIB3_TO_TIB01: OnceLock<bool> = OnceLock::new();
    let enabled: bool = *FORCE_TIB3_TO_TIB01.get_or_init(|| {
        std::env::var("RA2_FORCE_TIB3_TO_TIB01")
            .ok()
            .map(|v| {
                let n = v.trim().to_ascii_lowercase();
                n == "1" || n == "true" || n == "yes" || n == "on"
            })
            .unwrap_or(false)
    });
    if enabled && name.eq_ignore_ascii_case("TIB3_20") {
        Cow::Borrowed("TIB01")
    } else {
        Cow::Borrowed(name)
    }
}

fn is_resource_overlay_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("TIB") || upper.starts_with("GEM")
}

fn tiberium_id_offset() -> isize {
    static TIB_ID_OFFSET: OnceLock<isize> = OnceLock::new();
    *TIB_ID_OFFSET.get_or_init(|| {
        std::env::var("RA2_TIB_ID_OFFSET")
            .ok()
            .and_then(|s| s.parse::<isize>().ok())
            .unwrap_or(0)
    })
}

/// Resolve overlay name for rendering/debug display with optional resource-only ID offset.
///
/// `RA2_TIB_ID_OFFSET=N` applies only when the base ID resolves to TIB*/GEM* and
/// the shifted target also resolves to TIB*/GEM*. This avoids shifting bridges/rocks.
pub fn resolve_overlay_name_for_render(
    reg: &OverlayTypeRegistry,
    overlay_id: u8,
) -> Option<String> {
    let base_name = reg.name(overlay_id)?;
    let mut resolved_id: u8 = overlay_id;
    let offset = tiberium_id_offset();
    if offset != 0 && is_resource_overlay_name(base_name) {
        let shifted = overlay_id as isize + offset;
        if (0..=u8::MAX as isize).contains(&shifted) {
            let shifted_id = shifted as u8;
            if let Some(candidate) = reg.name(shifted_id) {
                if is_resource_overlay_name(candidate) {
                    resolved_id = shifted_id;
                }
            }
        }
    }
    reg.name(resolved_id)
        .map(|n| remap_overlay_name_for_debug(n).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shp_candidates() {
        let names: Vec<String> = overlay_shp_candidates("GEM01", "tem");
        assert_eq!(names[0], "gem01.tem");
        assert_eq!(names[1], "gem01.shp");
        assert_eq!(names[2], "GEM01.tem");
        assert_eq!(names[3], "GEM01.shp");
    }
}
