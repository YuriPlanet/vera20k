//! `[Colors]` color-scheme parsing and the gamemd HSV→RGB conversion, shared by
//! the loading screen and house-color ramp construction.
//!
//! Each `[Colors]` entry is `Name=H,S,V` (three 0..=255 bytes). gamemd keeps that
//! H,S,V triple on the color-scheme object and converts it to RGB through a fixed
//! 6-sextant integer routine when filling the loading bar's empty backing. This
//! module implements that conversion; sampled native comparisons accompany it.
//!
//! The loading screen consumes the parsed schemes directly. Unit/building/radar
//! colors use the ramps built by [`crate::rules::house_colors`], which shares the
//! HSV→RGB helper.
//!
//! ## Dependency rules
//! - Part of rules/ — depends only on rules/ini_parser. No sim/render/ui deps.

use crate::rules::ini_parser::IniFile;

/// One `[Colors]` entry: a name and its H,S,V triple (each 0..=255), kept in
/// section declaration order. Order is load-bearing: a player's color priority
/// maps to a runtime scheme index through [`PRIORITY_TO_SCHEME_INDEX`], and that
/// index addresses the (doubled) scheme list built from this section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorSchemeEntry {
    pub name: String,
    pub hsv: [u8; 3],
}

/// Player color priority (lobby slot 0..=7, plus the trailing default) → runtime
/// color-scheme array index. Matches gamemd's SessionClass priority table. The 8
/// multiplayer colors land on the scattered indices below; index 8 is the default
/// also used for the random/observer priority.
pub const PRIORITY_TO_SCHEME_INDEX: [usize; 9] = [3, 11, 21, 29, 13, 25, 17, 15, 5];

/// Scheme index for priority `-2` (random / no explicit color choice).
pub const RANDOM_PRIORITY_SCHEME_INDEX: usize = 5;

/// Map a color priority to a runtime scheme-array index, matching gamemd's
/// SessionClass priority→scheme conversion: `-2` → a fixed default, `0..=8` go
/// through the table, anything higher passes through unchanged.
pub fn scheme_index_for_priority(priority: i32) -> usize {
    if priority == -2 {
        RANDOM_PRIORITY_SCHEME_INDEX
    } else if (0..PRIORITY_TO_SCHEME_INDEX.len() as i32).contains(&priority) {
        PRIORITY_TO_SCHEME_INDEX[priority as usize]
    } else {
        priority.max(0) as usize
    }
}

/// `[Colors]` entry index for a color priority (the runtime scheme index, un-doubled): the priority
/// LUT result divided by 2 (runtime scheme `R` addresses `[Colors]` entry `R / 2`).
pub fn scheme_entry_for_priority(priority: i32) -> usize {
    scheme_index_for_priority(priority) / 2
}

/// `[Colors]` entry index for a map `Color=<name>` (case-insensitive). `None` if no entry matches.
pub fn scheme_entry_by_name(schemes: &[ColorSchemeEntry], name: &str) -> Option<usize> {
    let want = name.trim();
    schemes
        .iter()
        .position(|s| s.name.eq_ignore_ascii_case(want))
}

/// H,S,V of a `[Colors]` entry by index.
pub fn scheme_hsv_by_entry(schemes: &[ColorSchemeEntry], entry: usize) -> Option<[u8; 3]> {
    schemes.get(entry).map(|s| s.hsv)
}

/// Parse the `[Colors]` section into entries in declaration order.
///
/// Values are `H,S,V`; a malformed/short line is skipped rather than aborting so
/// one bad entry can't desync the rest of the list (which would shift every
/// later scheme index).
pub fn parse_color_schemes(ini: &IniFile) -> Vec<ColorSchemeEntry> {
    let Some(section) = ini.section("Colors") else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    section
        .keys()
        .filter_map(|key| {
            if !seen.insert(key.to_ascii_uppercase()) {
                return None;
            }
            let value = section.get(key)?;
            let hsv = parse_hsv_triple(value)?;
            Some(ColorSchemeEntry {
                name: key.to_string(),
                hsv,
            })
        })
        .collect()
}

/// Parse `H,S,V` (three 0..=255 bytes). The INI parser already strips inline
/// comments, but cut at `;` defensively in case a raw value reaches here.
fn parse_hsv_triple(value: &str) -> Option<[u8; 3]> {
    let head = value.split(';').next().unwrap_or(value);
    let mut parts = head.split(',').map(str::trim);
    let h = parts.next()?.parse::<u8>().ok()?;
    let s = parts.next()?.parse::<u8>().ok()?;
    let v = parts.next()?.parse::<u8>().ok()?;
    Some([h, s, v])
}

/// Resolve the loading-bar color scheme for a player color priority.
///
/// gamemd builds two runtime schemes per `[Colors]` entry (a documented quirk:
/// the schemes "are all doubled"), so runtime scheme index `N` addresses
/// `[Colors]` list entry `N / 2`; both halves of a pair share the same source
/// H,S,V. The priority table selects the odd (`2*entry + 1`) half.
pub fn scheme_for_priority(
    schemes: &[ColorSchemeEntry],
    priority: i32,
) -> Option<&ColorSchemeEntry> {
    schemes.get(scheme_index_for_priority(priority) / 2)
}

/// gamemd's 6-sextant integer HSV→RGB. H, S, V and each output channel are
/// 0..=255. All divisions truncate, matching the binary's integer arithmetic.
/// Native: `HSV_To_RGB` 0x00517440, called by loading fill at 0x006434B5
/// and house-ramp construction at 0x0068C475. Body/callers rechecked 2026-09-08;
/// `tools/color_oracle/hsv_to_rgb.py` compares sampled original executable output.
pub fn hsv_to_rgb(hsv: [u8; 3]) -> [u8; 3] {
    let h = hsv[0] as u32;
    let s = hsv[1] as u32;
    let v = hsv[2] as u32;
    // `f` is the position within the current sextant (0..254); `region` is the
    // sextant index. H=255 yields region 6 with f=0, which falls through to the
    // red wrap (same output as region 0, since t collapses to p when f=0).
    let f = (h * 6) % 255;
    let region = (h * 6) / 255;
    let p = ((255 - s) * v) / 255;
    let q = ((255 - (f * s) / 255) * v) / 255;
    let t = ((255 - ((255 - f) * s) / 255) * v) / 255;
    let (r, g, b) = match region {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        5 => (v, p, q),
        _ => (v, t, p),
    };
    [r as u8, g as u8, b as u8]
}

/// Resolve a player color priority to the loading-bar backing color in RGB,
/// applying the priority table, the scheme-doubling, and HSV→RGB. Returns `None`
/// when the `[Colors]` list is empty or the index is out of range.
#[cfg(test)]
pub fn backing_rgb_for_priority(schemes: &[ColorSchemeEntry], priority: i32) -> Option<[u8; 3]> {
    scheme_for_priority(schemes, priority).map(|scheme| hsv_to_rgb(scheme.hsv))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_to_rgb_matches_native_oracle_all_hues_at_sv_boundaries() {
        #[derive(serde::Deserialize)]
        struct Vectors {
            cases: Vec<Case>,
        }
        #[derive(serde::Deserialize)]
        struct Case {
            saturation: u8,
            value: u8,
            rgb_by_hue: String,
        }
        let vectors: Vectors =
            serde_json::from_str(include_str!("../../tools/color_oracle/hsv_to_rgb.json"))
                .expect("native HSV vectors");
        // Require the intended sample, not merely whichever rows remain in the file.
        let pairs = [
            (0, 255),
            (1, 255),
            (127, 128),
            (128, 127),
            (254, 255),
            (255, 0),
            (255, 1),
            (255, 254),
            (255, 255),
        ];
        assert_eq!(vectors.cases.len(), pairs.len());
        for (case, pair) in vectors.cases.iter().zip(pairs) {
            assert_eq!((case.saturation, case.value), pair);
            assert_eq!(case.rgb_by_hue.len(), 256 * 3 * 2);
            for hue in 0..=255u8 {
                let offset = usize::from(hue) * 6;
                let expected: [u8; 3] = std::array::from_fn(|channel| {
                    let begin = offset + channel * 2;
                    u8::from_str_radix(&case.rgb_by_hue[begin..begin + 2], 16)
                        .expect("native RGB byte")
                });
                let hsv = [hue, case.saturation, case.value];
                assert_eq!(hsv_to_rgb(hsv), expected, "native HSV mismatch at {hsv:?}");
            }
        }
    }

    fn schemes() -> Vec<ColorSchemeEntry> {
        // The retail rulesmd `[Colors]` list, in order. Only the entries the
        // priority table can reach (and a couple of anchors) need to be exact.
        let raw: &[(&str, [u8; 3])] = &[
            ("LightGold", [25, 255, 255]),  // 0
            ("Gold", [43, 239, 255]),       // 1   priority 0
            ("LightGrey", [0, 0, 240]),     // 2   priority -2 default (white)
            ("Grey", [0, 0, 131]),          // 3
            ("Red", [20, 255, 184]),        // 4
            ("DarkRed", [0, 230, 255]),     // 5   priority 1
            ("Orange", [25, 230, 255]),     // 6   priority 4
            ("Magenta", [221, 102, 255]),   // 7   priority 7
            ("Purple", [201, 201, 189]),    // 8   priority 6
            ("LightBlue", [119, 143, 255]), // 9
            ("DarkBlue", [153, 214, 212]),  // 10  priority 2
            ("NeonBlue", [185, 156, 238]),  // 11
            ("DarkSky", [131, 200, 230]),   // 12  priority 5
            ("Green", [104, 241, 195]),     // 13
            ("DarkGreen", [81, 200, 210]),  // 14  priority 3
        ];
        raw.iter()
            .map(|(name, hsv)| ColorSchemeEntry {
                name: name.to_string(),
                hsv: *hsv,
            })
            .collect()
    }

    #[test]
    fn hsv_to_rgb_matches_gamemd_sextants() {
        // DarkRed (H=0): pure red sextant.
        assert_eq!(hsv_to_rgb([0, 230, 255]), [255, 25, 25]);
        // DarkBlue (H=153): blue sextant, blue channel dominant.
        assert_eq!(hsv_to_rgb([153, 214, 212]), [34, 105, 212]);
        // Gold (H=43): yellow, red+green high.
        assert_eq!(hsv_to_rgb([43, 239, 255]), [253, 255, 16]);
        // Zero saturation is grey regardless of hue.
        assert_eq!(hsv_to_rgb([0, 0, 131]), [131, 131, 131]);
    }

    #[test]
    fn darkblue_reads_blue_dominant_darkred_red_dominant() {
        let blue = hsv_to_rgb([153, 214, 212]);
        assert!(blue[2] > blue[0] && blue[2] > blue[1], "{blue:?}");
        let red = hsv_to_rgb([0, 230, 255]);
        assert!(red[0] > red[1] && red[0] > red[2], "{red:?}");
    }

    #[test]
    fn priority_table_selects_the_eight_multiplayer_colors() {
        let schemes = schemes();
        // priority 0..=7 → the standard MP colors via the doubling (idx/2).
        let expect: &[(i32, &str)] = &[
            (0, "Gold"),
            (1, "DarkRed"),
            (2, "DarkBlue"),
            (3, "DarkGreen"),
            (4, "Orange"),
            (5, "DarkSky"),
            (6, "Purple"),
            (7, "Magenta"),
        ];
        for (priority, name) in expect {
            assert_eq!(
                scheme_for_priority(&schemes, *priority).map(|s| s.name.as_str()),
                Some(*name),
                "priority {priority}"
            );
        }
        // Random / -2 falls back to the white default (LightGrey, index 5 → 2).
        assert_eq!(
            scheme_for_priority(&schemes, -2).map(|s| s.name.as_str()),
            Some("LightGrey")
        );
    }

    #[test]
    fn backing_rgb_resolves_player_priority_to_scheme_color() {
        let schemes = schemes();
        // Priority 2 = DarkBlue → blue-dominant backing.
        let blue = backing_rgb_for_priority(&schemes, 2).unwrap();
        assert!(blue[2] > blue[0] && blue[2] > blue[1], "{blue:?}");
        // Priority 1 = DarkRed → red-dominant backing.
        let red = backing_rgb_for_priority(&schemes, 1).unwrap();
        assert!(red[0] > red[1] && red[0] > red[2], "{red:?}");
    }

    #[test]
    fn parse_color_schemes_reads_section_in_order() {
        let ini = IniFile::from_str("[Colors]\nGold=43,239,255\nDarkRed=0,230,255\n");
        let parsed = parse_color_schemes(&ini);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "Gold");
        assert_eq!(parsed[0].hsv, [43, 239, 255]);
        assert_eq!(parsed[1].hsv, [0, 230, 255]);
    }

    #[test]
    fn parse_color_schemes_find_or_create_keeps_first_identity() {
        let ini = IniFile::from_str("[Colors]\nGold=43,239,255\ngold=1,2,3\n");
        let parsed = parse_color_schemes(&ini);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "Gold");
        assert_eq!(parsed[0].hsv, [43, 239, 255]);
    }

    #[test]
    fn empty_or_missing_colors_section_yields_no_schemes() {
        assert!(parse_color_schemes(&IniFile::from_str("[General]\nX=1\n")).is_empty());
        assert!(backing_rgb_for_priority(&[], 0).is_none());
    }

    #[test]
    fn scheme_entry_for_priority_matches_lut_div2() {
        // p0→3/2=1 Gold, p1→11/2=5 DarkRed, p2→21/2=10 DarkBlue, p3→29/2=14 DarkGreen, random→5/2=2
        assert_eq!(scheme_entry_for_priority(0), 1);
        assert_eq!(scheme_entry_for_priority(1), 5);
        assert_eq!(scheme_entry_for_priority(2), 10);
        assert_eq!(scheme_entry_for_priority(3), 14);
        assert_eq!(scheme_entry_for_priority(-2), 2);
    }

    #[test]
    fn scheme_entry_by_name_and_hsv_lookup() {
        let s = schemes();
        assert_eq!(scheme_entry_by_name(&s, "darkred"), Some(5));
        assert_eq!(scheme_entry_by_name(&s, "DarkBlue"), Some(10));
        assert_eq!(scheme_entry_by_name(&s, "Nope"), None);
        assert_eq!(scheme_hsv_by_entry(&s, 5), Some([0, 230, 255])); // DarkRed
    }
}
