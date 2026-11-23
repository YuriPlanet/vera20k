//! Headless asset browsing for automated callers, behind a machine-readable
//! interface — see the `asset` binary in `src/bin/asset.rs`.
//!
//! Archives are read-only. Explicit export verbs write separate files, including
//! candidate corpus baselines for review. What it reports about retail *file contents*
//! (dimensions, frame geometry, string values) is read straight from retail
//! bytes, but a render it produces is not parity evidence.
//!
//! ## Dependency rules
//! - May depend on `assets/`, `rules/`, `util/`, `map/` source selection, `skirmish_modes`, and the CPU-only parts of
//!   `render/` (bitmap font glyph data).
//! - Must NEVER depend on `sim/`, `ui/`, `audio/`, `net/`, or any
//!   `app*` module — and nothing in `sim/` may depend on this.

pub mod args;
pub mod canvas;
pub mod identify;
pub mod locate;
pub mod names;
pub mod palette;
pub mod palette_production;
pub mod render_dispatch;
pub mod render_still;
pub mod render_tmp;
pub mod render_vxl;
pub mod report;
pub mod retail_corpus;
pub mod root;
pub mod verb_art;
pub mod verb_compare;
pub mod verb_csf;
pub mod verb_extract;
pub mod verb_find;
pub mod verb_info;
pub mod verb_ini;
pub mod verb_ini_expand;
pub mod verb_ls;
pub mod verb_palette;
pub mod verb_parse_check;
pub mod verb_render;
pub mod verb_scan;
pub mod verb_sound;

/// Turns an asset or archive name into one output path component: ASCII
/// letters, digits and `.`, `_`, `-` stay, every other character becomes `_`,
/// and an empty name becomes the verb's own `fallback`. Flattening the path
/// separators is what keeps a name like `../../x` inside the output root, and
/// one rule keeps every verb writing the same asset under the same name.
pub fn sanitise_name(name: &str, fallback: &str) -> String {
    let sanitised: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitised.is_empty() {
        fallback.to_string()
    } else {
        sanitised
    }
}

#[cfg(all(test, tests_asset_tools))]
mod tests {
    use super::sanitise_name;

    #[test]
    fn sanitise_name_keeps_safe_characters_and_replaces_the_rest() {
        assert_eq!(sanitise_name("gi-idle_01.shp", "asset"), "gi-idle_01.shp");
        assert_eq!(
            sanitise_name("ra2\\dir/na me:x*?", "asset"),
            "ra2_dir_na_me_x__"
        );
        assert_eq!(sanitise_name("", "asset"), "asset");
        assert_eq!(sanitise_name("", "sound"), "sound");
    }
}
