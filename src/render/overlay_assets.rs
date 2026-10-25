//! Render-only debug overlay-name resolution over a rules `OverlayTypeRegistry`.
//! Split out of the overlay type registry (F04); presentation-only — no sim or map
//! consumer may depend on the env-var debug remaps here.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::rules::overlay_types::OverlayTypeRegistry;

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
            if let Some(candidate) = reg.name(shifted_id)
                && is_resource_overlay_name(candidate)
            {
                resolved_id = shifted_id;
            }
        }
    }
    reg.name(resolved_id)
        .map(|n| remap_overlay_name_for_debug(n).into_owned())
}

/// The render name of every registered overlay type, keyed by its
/// `[OverlayTypes]` index. `CellClass::DrawOverlay_Body @ 0x0047F6A0` and
/// `DrawOverlay_Shadow @ 0x0047F510` take the type straight from
/// `OverlayTypeClass::Array[Cell+0x44]`, so every identity a cell can hold,
/// authored or written at runtime, resolves here; nothing is pre-selected.
pub fn render_overlay_names(reg: &OverlayTypeRegistry) -> BTreeMap<u8, String> {
    (0..=u8::MAX)
        .filter_map(|overlay_id| {
            Some((
                overlay_id,
                resolve_overlay_name_for_render(reg, overlay_id)?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_overlay_names_cover_every_registered_type() {
        // No kind or map entry pre-selects an identity: walls, crates, ore
        // variants and rubble written after load are named like any other.
        let mut text = String::from("[OverlayTypes]\n");
        for overlay_id in 0..=0xEFu8 {
            text.push_str(&format!("{overlay_id}=TYPE{overlay_id}\n"));
        }
        let reg = OverlayTypeRegistry::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(&text),
            None,
        );

        let names = render_overlay_names(&reg);

        assert_eq!(names.len(), 0xF0);
        for (&overlay_id, name) in &names {
            assert_eq!(Some(name.as_str()), reg.name(overlay_id));
        }
        assert!(!names.contains_key(&0xF0), "unregistered index");
    }
}
